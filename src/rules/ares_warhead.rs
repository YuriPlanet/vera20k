//! Retained WarheadExt configuration shared by the Ares EMP and AttachEffect
//! receivers. Installed 10051890 owns defaults; 10051B70 owns House admission.
//! The retained processing registry reads it; RuleSet projects an immutable view.
#[cfg(any(feature = "ares", feature = "phobos"))]
use crate::rules::ini_parser::IniSection;
use crate::util::native_x87::NativeF64Bits;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresWarheadRules {
    duration: i32,
    cap: i32,
    sparkles: Option<String>,
    affects_enemies: bool,
    affects_owner: Option<bool>,
    attach_effect: AttachEffectRules,
    #[cfg(feature = "ares")]
    #[serde(default)]
    kill_driver: KillDriverRules,
    #[cfg(feature = "ares")]
    #[serde(default)]
    effects_require_damage: bool,
    #[cfg(feature = "ares")]
    #[serde(default = "default_effects_require_verses")]
    effects_require_verses: bool,
    #[cfg(feature = "ares")]
    #[serde(default)]
    combat: WarheadCombatRules,
}
impl Default for AresWarheadRules {
    fn default() -> Self {
        Self {
            duration: 0,
            cap: -1,
            sparkles: None,
            affects_enemies: true,
            affects_owner: None,
            attach_effect: Default::default(),
            #[cfg(feature = "ares")]
            kill_driver: Default::default(),
            #[cfg(feature = "ares")]
            effects_require_damage: false,
            #[cfg(feature = "ares")]
            effects_require_verses: true,
            #[cfg(feature = "ares")]
            combat: Default::default(),
        }
    }
}
impl AresWarheadRules {
    pub(crate) fn duration(&self) -> i32 {
        self.duration
    }
    pub(crate) fn cap(&self) -> i32 {
        self.cap
    }
    pub(crate) fn can_affect_house(
        &self,
        source_exists: bool,
        same_owner: bool,
        allied: bool,
        affects_allies: bool,
    ) -> bool {
        // Installed WarheadExt10051B70: the source tests the target's bit.
        !source_exists
            || if same_owner && self.affects_owner.is_some() {
                self.affects_owner.expect("present owner override")
            } else if allied {
                affects_allies
            } else {
                self.affects_enemies
            }
    }
    pub(crate) fn sparkles(&self) -> Option<&str> {
        self.sparkles.as_deref()
    }
    pub(crate) fn attach_effect(&self) -> &AttachEffectRules {
        &self.attach_effect
    }
    pub(crate) fn affects_enemies(&self) -> bool {
        self.affects_enemies
    }
    pub(crate) fn affects_owner(&self) -> Option<bool> {
        self.affects_owner
    }
    #[cfg(feature = "ares")]
    pub(crate) fn kill_driver(&self) -> &KillDriverRules {
        &self.kill_driver
    }
    #[cfg(feature = "ares")]
    pub(crate) fn effects_require_damage(&self) -> bool {
        self.effects_require_damage
    }
    #[cfg(feature = "ares")]
    pub(crate) fn effects_require_verses(&self) -> bool {
        self.effects_require_verses
    }
    #[cfg(feature = "ares")]
    pub(crate) fn combat(&self) -> &WarheadCombatRules {
        &self.combat
    }
    /// The two NullableIdx<Voc> overrides bind against the fixed sound
    /// catalog after the type passes, like every other Voc reference.
    #[cfg(feature = "ares")]
    pub(crate) fn combat_mut(&mut self) -> &mut WarheadCombatRules {
        &mut self.combat
    }
    #[cfg(feature = "ares")]
    pub(crate) fn read_pass(
        &mut self,
        section: &IniSection,
        sparkle: Option<Option<String>>,
        animation: Option<Option<String>>,
    ) {
        let _ares = crate::rules::ares_switch::ares_reads();
        self.duration = section.read_ares_int("EMP.Duration", self.duration);
        self.cap = section.read_ares_int("EMP.Cap", self.cap);
        if let Some(sparkle) = sparkle {
            self.sparkles = sparkle;
        }
        self.affects_enemies = section.read_ares_bool("AffectsEnemies", self.affects_enemies);
        self.affects_owner = section.read_ares_nullable_bool("AffectsOwner", self.affects_owner);
        self.kill_driver.read_pass(section);
        // WarheadExt+130/+131, ctor10051A8E=false/true; reads1005231D/33.
        self.effects_require_damage =
            section.read_ares_bool("EffectsRequireDamage", self.effects_require_damage);
        self.effects_require_verses =
            section.read_ares_bool("EffectsRequireVerses", self.effects_require_verses);
        self.attach_effect.read_pass(section, animation);
    }
}

#[cfg(feature = "ares")]
fn default_effects_require_verses() -> bool {
    true
}

/// Installed house-selector table10095264, reader1000A660 and shared
/// resolver10021310. Phobos' `read<OwnerHouseKind>` (`Utilities/TemplateDef.h`)
/// matches the same eight names. The selector's random permission is a
/// consumer input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[cfg(any(feature = "ares", feature = "phobos"))]
pub(crate) enum HouseSelector {
    Default,
    Invoker,
    Killer,
    Victim,
    Civilian,
    Special,
    Neutral,
    Random,
}

#[cfg(any(feature = "ares", feature = "phobos"))]
impl HouseSelector {
    pub(crate) fn read(self, section: &IniSection, key: &str) -> Self {
        let _ares = crate::rules::ares_switch::ares_reads();
        const VALUES: [(&str, HouseSelector); 8] = [
            ("default", HouseSelector::Default),
            ("invoker", HouseSelector::Invoker),
            ("killer", HouseSelector::Killer),
            ("victim", HouseSelector::Victim),
            ("civilian", HouseSelector::Civilian),
            ("special", HouseSelector::Special),
            ("neutral", HouseSelector::Neutral),
            ("random", HouseSelector::Random),
        ];
        section.read_string_with(key, 2048, self, |current, raw| {
            VALUES
                .iter()
                .find_map(|(name, value)| name.eq_ignore_ascii_case(raw).then_some(*value))
                .unwrap_or(current)
        })
    }
}

/// WarheadExt+5D..74, constructor1005194B/1005194E/10051A70/10051A77.
/// ReadINI1005207E..100520D8 uses the exact full keys, without clamps.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[cfg(feature = "ares")]
pub(crate) struct KillDriverRules {
    enabled: bool,
    kill_below_percent: NativeF64Bits,
    chance: NativeF64Bits,
    owner: HouseSelector,
    remove_veterancy: bool,
}

#[cfg(feature = "ares")]
impl Default for KillDriverRules {
    fn default() -> Self {
        Self {
            enabled: false,
            kill_below_percent: NativeF64Bits::ONE,
            chance: NativeF64Bits::ONE,
            owner: HouseSelector::Special,
            remove_veterancy: false,
        }
    }
}

#[cfg(feature = "ares")]
impl KillDriverRules {
    #[cfg(feature = "ares")]
    fn read_pass(&mut self, section: &IniSection) {
        let _ares = crate::rules::ares_switch::ares_reads();
        self.enabled = section.read_ares_bool("KillDriver", self.enabled);
        self.kill_below_percent =
            section.read_ares_double_bits("KillDriver.KillBelowPercent", self.kill_below_percent);
        self.chance = section.read_ares_double_bits("KillDriver.Chance", self.chance);
        self.owner = self.owner.read(section, "KillDriver.Owner");
        self.remove_veterancy =
            section.read_ares_bool("KillDriver.RemoveVeterancy", self.remove_veterancy);
    }
    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }
    pub(crate) fn kill_below_percent(&self) -> NativeF64Bits {
        self.kill_below_percent
    }
    pub(crate) fn chance(&self) -> NativeF64Bits {
        self.chance
    }
    pub(crate) fn owner(&self) -> HouseSelector {
        self.owner
    }
    pub(crate) fn remove_veterancy(&self) -> bool {
        self.remove_veterancy
    }
}

/// The RulesExt keys the warhead hooks read: LoadBeforeTypeData 1002AB60
/// reads both with the Valueable<bool> reader on every Rules pass.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[cfg(feature = "ares")]
pub(crate) struct WarheadGlobalRules {
    /// RulesExt+0x111 `[General] DamageAirConsiderBridges`, constructor false.
    damage_air_consider_bridges: bool,
    /// RulesExt+0x14D `[AudioVisual] IronCurtainFlash`, constructor true.
    iron_curtain_flash: bool,
}

#[cfg(feature = "ares")]
impl Default for WarheadGlobalRules {
    fn default() -> Self {
        Self {
            damage_air_consider_bridges: false,
            iron_curtain_flash: true,
        }
    }
}

#[cfg(feature = "ares")]
impl WarheadGlobalRules {
    pub(crate) fn read_before_type_data(&mut self, pass: &crate::rules::ini_parser::IniFile) {
        let _ares = crate::rules::ares_switch::ares_reads();
        if let Some(section) = pass.section("General") {
            self.damage_air_consider_bridges = section
                .read_ares_bool("DamageAirConsiderBridges", self.damage_air_consider_bridges);
        }
        if let Some(section) = pass.section("AudioVisual") {
            self.iron_curtain_flash =
                section.read_ares_bool("IronCurtainFlash", self.iron_curtain_flash);
        }
    }
    pub(crate) fn damage_air_consider_bridges(&self) -> bool {
        self.damage_air_consider_bridges
    }
    pub(crate) fn iron_curtain_flash(&self) -> bool {
        self.iron_curtain_flash
    }
}

/// Victim kinds of WarheadTypeExt::GetRelativeDamage (installed 10051C50),
/// in RelativeDamage.* field order (+E4..+F4).
#[cfg(feature = "ares")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RelativeDamageKind {
    Buildings,
    Aircraft,
    Infantry,
    Vehicles,
    Terrain,
}

/// Ares 3.0 WarheadTypeExt damage modifiers and kill presentation. The
/// constructor 10051890 owns the defaults; ReadINI 10051D80 reads each key
/// once per Rules pass with the Valueable/Nullable readers named below.
/// Every key is independent, so the read order inside this group is not
/// observable; `IonCannon` and the other effect keys this struct lacks are
/// not read.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[cfg(feature = "ares")]
pub(crate) struct WarheadCombatRules {
    /// +0x08 `MindControl.Permanent` (detail::read<bool>).
    #[serde(default)]
    mind_control_permanent: bool,
    /// +0x18 `IronCurtain.Duration` and +0x1C `IronCurtain.Cap`
    /// (detail::read<int>).
    #[serde(default)]
    iron_curtain_duration: i32,
    #[serde(default = "default_iron_curtain_cap")]
    iron_curtain_cap: i32,
    /// +0x4C Valueable<AnimTypeClass*> `InfDeathAnim` (Find only; `none`
    /// clears).
    #[serde(default)]
    inf_death_anim: Option<String>,
    /// +0xF8 `Sonar.Duration` (Valueable<int>, constructor 0).
    #[serde(default)]
    sonar_duration: i32,
    /// +0xFC `DisableWeapons.Duration`.
    #[serde(default)]
    disable_weapons_duration: i32,
    /// +0x104..+0x128, the `IonCannon` group.
    #[serde(default)]
    ion_cannon: IonCannonRules,
    /// +0x20 Nullable<bool> `IronCurtain.Flash`.
    iron_curtain_flash: Option<bool>,
    /// +0x38 Valueable<double> `Temporal.HealthFactor`.
    temporal_health_factor: NativeF64Bits,
    /// +0x40 Nullable<AnimTypeClass*> `Temporal.WarpAway`: `Some(None)` is
    /// a successful `none`.
    temporal_warp_away: Option<Option<String>>,
    /// +0x30 `Damage.Deployed`, detail::read<double> (binary64 scan).
    deployed_damage: NativeF64Bits,
    /// +0x75 `Malicious`.
    malicious: bool,
    /// +0x76 `PreventScatter`.
    prevent_scatter: bool,
    /// +0x77 Nullable<bool> `BridgeAbsoluteDestroyer`.
    bridge_absolute_destroyer: Option<bool>,
    /// +0x7C `CellSpread.MaxAffect`.
    cell_spread_max_affect: i32,
    /// +0x80 `DamageAirThreshold`.
    damage_air_threshold: i32,
    /// +0xD0 `UnitLost.Suppress`.
    unit_lost_suppress: bool,
    /// +0xD1/+0xD2 `DeathWeapon.SuppressVehicles`/`.SuppressInfantry`.
    death_weapon_suppress_vehicles: bool,
    death_weapon_suppress_infantry: bool,
    /// +0xD4 ValueableVector<TechnoTypeClass*> `DeathWeapon.Suppress`.
    death_weapon_suppress: Vec<crate::rules::superweapon_type::TechnoTypeReference>,
    /// +0xE0 `RelativeDamage` and +0xE4..+0xF4 its five kinds.
    relative_damage: bool,
    relative_damage_by_kind: [i32; 5],
    /// +0x100 `Flash.Duration`.
    flash_duration: i32,
    /// +0x132 `AllowZeroDamage`.
    allow_zero_damage: bool,
    /// +0x134/+0x13C NullableIdx<VocClass> `DieSound.Override`/
    /// `VoiceDie.Override`: `None` unset, `Some(None)` the -1 index.
    die_sound_override: Option<Option<String>>,
    voice_die_override: Option<Option<String>>,
    /// +0x144/+0x150 Promotable<int> `Culling.%sBelowHealth`/
    /// `Culling.%sChance`, Rookie/Veteran/Elite.
    culling_below_health: [i32; 3],
    culling_chance: [i32; 3],
    /// +0x50 Nullable<int> `NukeFlash.Duration`, +0x58 ValueableIdx<Anim>
    /// `PreImpactAnim` (constructor -1: none) and +0x5C `PreImpactAnim.Moves`,
    /// which the Bullet hooks 467E59 (10036960) and 46670F (1001AC10) read.
    #[serde(default)]
    nuke_flash_duration: Option<i32>,
    #[serde(default)]
    pre_impact_anim: Option<String>,
    #[serde(default)]
    pre_impact_anim_moves: bool,
}

#[cfg(feature = "ares")]
impl Default for WarheadCombatRules {
    fn default() -> Self {
        Self {
            mind_control_permanent: false,
            iron_curtain_duration: 0,
            iron_curtain_cap: default_iron_curtain_cap(),
            inf_death_anim: None,
            sonar_duration: 0,
            disable_weapons_duration: 0,
            ion_cannon: IonCannonRules::default(),
            iron_curtain_flash: None,
            temporal_health_factor: NativeF64Bits::from_bits(0),
            temporal_warp_away: None,
            deployed_damage: NativeF64Bits::ONE,
            malicious: true,
            prevent_scatter: false,
            bridge_absolute_destroyer: None,
            cell_spread_max_affect: -1,
            damage_air_threshold: 0,
            unit_lost_suppress: false,
            death_weapon_suppress_vehicles: false,
            death_weapon_suppress_infantry: false,
            death_weapon_suppress: Vec::new(),
            relative_damage: false,
            relative_damage_by_kind: [0; 5],
            flash_duration: 0,
            allow_zero_damage: false,
            die_sound_override: None,
            voice_die_override: None,
            // Constructor 10051890: Culling_BelowHealth(0) with Elite (+0x14C)
            // then set to -1; Culling_Chance(-1) for all three ranks.
            culling_below_health: [0, 0, -1],
            culling_chance: [-1; 3],
            nuke_flash_duration: None,
            pre_impact_anim: None,
            pre_impact_anim_moves: false,
        }
    }
}

#[cfg(feature = "ares")]
fn default_iron_curtain_cap() -> i32 {
    -1
}

#[cfg(feature = "ares")]
impl WarheadCombatRules {
    /// The ReadINI 10051D80 reads of this group. `temporal_warp_away` is the
    /// pass's resolved `Temporal.WarpAway` (Nullable<Anim> 1002D910: unknown
    /// retains, `none` stores null), `inf_death_anim` its `InfDeathAnim`
    /// (the Valueable<Anim> Find read: unknown retains, `none` clears) and
    /// `death_weapon_suppress` its `DeathWeapon.Suppress` list
    /// (ValueableVector 1000EDD0: a reached read replaces, unknown names are
    /// skipped).
    pub(crate) fn read_pass(
        &mut self,
        section: &IniSection,
        temporal_warp_away: Option<Option<String>>,
        inf_death_anim: Option<Option<String>>,
        death_weapon_suppress: Option<Vec<crate::rules::superweapon_type::TechnoTypeReference>>,
    ) {
        let _ares = crate::rules::ares_switch::ares_reads();
        self.mind_control_permanent =
            section.read_ares_bool("MindControl.Permanent", self.mind_control_permanent);
        self.iron_curtain_duration =
            section.read_ares_int("IronCurtain.Duration", self.iron_curtain_duration);
        self.iron_curtain_cap = section.read_ares_int("IronCurtain.Cap", self.iron_curtain_cap);
        if let Some(anim) = inf_death_anim {
            self.inf_death_anim = anim;
        }
        // LoadFromINIFile reads Sonar.Duration right before DisableWeapons.
        self.sonar_duration = section.read_ares_int("Sonar.Duration", self.sonar_duration);
        self.disable_weapons_duration =
            section.read_ares_int("DisableWeapons.Duration", self.disable_weapons_duration);
        self.iron_curtain_flash =
            section.read_ares_nullable_bool("IronCurtain.Flash", self.iron_curtain_flash);
        self.temporal_health_factor =
            section.read_ares_double_bits("Temporal.HealthFactor", self.temporal_health_factor);
        if temporal_warp_away.is_some() {
            self.temporal_warp_away = temporal_warp_away;
        }
        self.deployed_damage =
            section.read_ares_double_bits("Damage.Deployed", self.deployed_damage);
        self.malicious = section.read_ares_bool("Malicious", self.malicious);
        self.prevent_scatter = section.read_ares_bool("PreventScatter", self.prevent_scatter);
        self.bridge_absolute_destroyer = section
            .read_ares_nullable_bool("BridgeAbsoluteDestroyer", self.bridge_absolute_destroyer);
        self.cell_spread_max_affect =
            section.read_ares_int("CellSpread.MaxAffect", self.cell_spread_max_affect);
        self.damage_air_threshold =
            section.read_ares_int("DamageAirThreshold", self.damage_air_threshold);
        self.unit_lost_suppress =
            section.read_ares_bool("UnitLost.Suppress", self.unit_lost_suppress);
        self.death_weapon_suppress_vehicles = section.read_ares_bool(
            "DeathWeapon.SuppressVehicles",
            self.death_weapon_suppress_vehicles,
        );
        self.death_weapon_suppress_infantry = section.read_ares_bool(
            "DeathWeapon.SuppressInfantry",
            self.death_weapon_suppress_infantry,
        );
        if let Some(list) = death_weapon_suppress {
            self.death_weapon_suppress = list;
        }
        self.relative_damage = section.read_ares_bool("RelativeDamage", self.relative_damage);
        for (slot, key) in [
            "RelativeDamage.Buildings",
            "RelativeDamage.Aircraft",
            "RelativeDamage.Infantry",
            "RelativeDamage.Vehicles",
            "RelativeDamage.Terrain",
        ]
        .into_iter()
        .enumerate()
        {
            self.relative_damage_by_kind[slot] =
                section.read_ares_int(key, self.relative_damage_by_kind[slot]);
        }
        self.flash_duration = section.read_ares_int("Flash.Duration", self.flash_duration);
        self.allow_zero_damage = section.read_ares_bool("AllowZeroDamage", self.allow_zero_damage);
        self.culling_below_health = read_promotable_int(
            section,
            ["Culling.BelowHealth", "Culling.RookieBelowHealth"],
            ["Culling.VeteranBelowHealth", "Culling.EliteBelowHealth"],
            self.culling_below_health,
        );
        self.culling_chance = read_promotable_int(
            section,
            ["Culling.Chance", "Culling.RookieChance"],
            ["Culling.VeteranChance", "Culling.EliteChance"],
            self.culling_chance,
        );
    }

    /// The `NUKE` Initialize of Extension<WarheadTypeClass>::LoadFromINI
    /// 10051D10's first call (with or without a section; the caller keeps
    /// that state): `PreImpactAnim` = FindIndex("NUKEBALL") and
    /// `NukeFlash.Duration` = 30.
    pub(crate) fn initialize_nuke(&mut self, nuke_ball: Option<String>) {
        self.pre_impact_anim = nuke_ball;
        self.nuke_flash_duration = Some(30);
    }

    /// LoadFromINIFile 10051D80's NukeFlash.Duration, PreImpactAnim (a
    /// ValueableIdx: a found name or `none` replaces, an unknown keeps) and
    /// PreImpactAnim.Moves.
    pub(crate) fn read_nuke_pass(
        &mut self,
        section: &IniSection,
        pre_impact_anim: Option<Option<String>>,
    ) {
        let _ares = crate::rules::ares_switch::ares_reads();
        self.nuke_flash_duration =
            section.read_ares_nullable_int("NukeFlash.Duration", self.nuke_flash_duration);
        if let Some(anim) = pre_impact_anim {
            self.pre_impact_anim = anim;
        }
        self.pre_impact_anim_moves =
            section.read_ares_bool("PreImpactAnim.Moves", self.pre_impact_anim_moves);
    }

    /// `NukeFlash.Duration`, unset or a frame count.
    pub(crate) fn nuke_flash_duration(&self) -> Option<i32> {
        self.nuke_flash_duration
    }

    /// `PreImpactAnim`, none for index -1.
    pub(crate) fn pre_impact_anim(&self) -> Option<&str> {
        self.pre_impact_anim.as_deref()
    }

    /// `PreImpactAnim.Moves`.
    pub(crate) fn pre_impact_anim_moves(&self) -> bool {
        self.pre_impact_anim_moves
    }

    /// The Voc overrides, read with NullableIdx<Voc> 1000E9D0.
    pub(crate) fn bind_sound_overrides(
        &mut self,
        section: Option<&IniSection>,
        sounds: &crate::rules::sound_ini::SoundRegistry,
    ) {
        let _ares = crate::rules::ares_switch::ares_reads();
        self.die_sound_override =
            section.and_then(|section| sounds.read_ares_reference(section, "DieSound.Override"));
        self.voice_die_override =
            section.and_then(|section| sounds.read_ares_reference(section, "VoiceDie.Override"));
    }

    pub(crate) fn mind_control_permanent(&self) -> bool {
        self.mind_control_permanent
    }
    /// `IronCurtain.Duration` and `IronCurtain.Cap`.
    pub(crate) fn iron_curtain(&self) -> (i32, i32) {
        (self.iron_curtain_duration, self.iron_curtain_cap)
    }
    pub(crate) fn inf_death_anim(&self) -> Option<&str> {
        self.inf_death_anim.as_deref()
    }
    pub(crate) fn sonar_duration(&self) -> i32 {
        self.sonar_duration
    }
    pub(crate) fn disable_weapons_duration(&self) -> i32 {
        self.disable_weapons_duration
    }
    pub(crate) fn ion_cannon(&self) -> &IonCannonRules {
        &self.ion_cannon
    }
    /// The pass reads the three type references of the group first.
    pub(crate) fn ion_cannon_mut(&mut self) -> &mut IonCannonRules {
        &mut self.ion_cannon
    }
    pub(crate) fn iron_curtain_flash(&self) -> Option<bool> {
        self.iron_curtain_flash
    }
    pub(crate) fn temporal_health_factor(&self) -> NativeF64Bits {
        self.temporal_health_factor
    }
    /// The erase anim: the Nullable's value, else `[General] WarpAway=`.
    pub(crate) fn temporal_warp_away<'a>(&'a self, rules_warp_away: &'a str) -> Option<&'a str> {
        match &self.temporal_warp_away {
            Some(anim) => anim.as_deref(),
            None => Some(rules_warp_away),
        }
    }
    pub(crate) fn deployed_damage(&self) -> NativeF64Bits {
        self.deployed_damage
    }
    pub(crate) fn malicious(&self) -> bool {
        self.malicious
    }
    pub(crate) fn prevent_scatter(&self) -> bool {
        self.prevent_scatter
    }
    /// DamageArea_BridgeAbsoluteDestroyer 10053880: the Nullable defaults to
    /// "this warhead is `[CombatDamage] IonCannonWarhead`".
    pub(crate) fn bridge_absolute_destroyer(&self, is_ion_cannon_warhead: bool) -> bool {
        self.bridge_absolute_destroyer
            .unwrap_or(is_ion_cannon_warhead)
    }
    pub(crate) fn cell_spread_max_affect(&self) -> i32 {
        self.cell_spread_max_affect
    }
    pub(crate) fn damage_air_threshold(&self) -> i32 {
        self.damage_air_threshold
    }
    pub(crate) fn unit_lost_suppress(&self) -> bool {
        self.unit_lost_suppress
    }
    /// TechnoClass_ReceiveDamage_SuppressDeathWeapon (hook 0x702669): Units
    /// by the vehicle flag, Infantry by the infantry flag, any type listed.
    pub(crate) fn suppresses_death_weapon(
        &self,
        category: crate::rules::object_type::ObjectCategory,
        type_id: &str,
    ) -> bool {
        use crate::rules::object_type::ObjectCategory;
        (category == ObjectCategory::Vehicle && self.death_weapon_suppress_vehicles)
            || (category == ObjectCategory::Infantry && self.death_weapon_suppress_infantry)
            || self.death_weapon_suppress.iter().any(|listed| {
                listed.category == category && listed.id.eq_ignore_ascii_case(type_id)
            })
    }
    /// GetRelativeDamage (installed 10051C50) when `RelativeDamage=yes`: the
    /// kind's value, positive as a percentage of the type's Strength,
    /// negative of the current Health (32-bit products, C division toward
    /// zero); 0 for a zero value, another kind (`None`) or no type.
    pub(crate) fn relative_damage(
        &self,
        kind: Option<RelativeDamageKind>,
        health: i32,
        strength: Option<i32>,
    ) -> Option<i32> {
        if !self.relative_damage {
            return None;
        }
        let value = kind.map_or(0, |kind| self.relative_damage_by_kind[kind as usize]);
        Some(match value {
            0 => 0,
            value if value < 0 => health.wrapping_mul(value).wrapping_div(-100),
            value => strength.map_or(0, |strength| strength.wrapping_mul(value) / 100),
        })
    }
    pub(crate) fn flash_duration(&self) -> i32 {
        self.flash_duration
    }
    pub(crate) fn allow_zero_damage(&self) -> bool {
        self.allow_zero_damage
    }
    pub(crate) fn die_sound_override(&self) -> Option<Option<&str>> {
        self.die_sound_override.as_ref().map(Option::as_deref)
    }
    pub(crate) fn voice_die_override(&self) -> Option<Option<&str>> {
        self.voice_die_override.as_ref().map(Option::as_deref)
    }
    /// Promotable<int>::Get for a rank index (Rookie 0, Veteran 1, Elite 2).
    pub(crate) fn culling(&self, rank: usize) -> (i32, i32) {
        (self.culling_below_health[rank], self.culling_chance[rank])
    }
}

/// WarheadTypeExt+0x104..+0x128: the ion blast an `IonCannon` or
/// `Ripple.Radius` warhead raises (applyRipples 10051AE0) and what its first
/// frame builds (IonBlastClass_Update_Beam 10053F10).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[cfg(feature = "ares")]
pub(crate) struct IonCannonRules {
    /// +0x104 Valueable<bool> `IonCannon`, constructor false.
    enabled: bool,
    /// +0x105 Valueable<bool> `IonCannon.Rock`, constructor true.
    rock: bool,
    /// +0x108 Nullable<int>, read from `Ripple.Radius`, then
    /// `IonCannon.Ripple` (both write the field).
    ripple_radius: Option<i32>,
    /// +0x110/+0x118 Nullable<AnimTypeClass*> `IonCannon.Blast`/`.Beam` and
    /// +0x120 Nullable<WarheadTypeClass*> `IonCannon.Warhead` (Find reads:
    /// `Some(None)` is a successful `none`).
    blast: Option<Option<String>>,
    beam: Option<Option<String>>,
    warhead: Option<Option<String>>,
    /// +0x128 Nullable<int> `IonCannon.Damage`.
    damage: Option<i32>,
}

#[cfg(feature = "ares")]
impl Default for IonCannonRules {
    fn default() -> Self {
        Self {
            enabled: false,
            rock: true,
            ripple_radius: None,
            blast: None,
            beam: None,
            warhead: None,
            damage: None,
        }
    }
}

#[cfg(feature = "ares")]
impl IonCannonRules {
    /// The ReadINI 10051D80 reads, given the pass's resolved references
    /// (`None` where the read failed and the field retains).
    pub(crate) fn read_pass(
        &mut self,
        section: &IniSection,
        blast: Option<Option<String>>,
        beam: Option<Option<String>>,
        warhead: Option<Option<String>>,
    ) {
        let _ares = crate::rules::ares_switch::ares_reads();
        self.enabled = section.read_ares_bool("IonCannon", self.enabled);
        self.rock = section.read_ares_bool("IonCannon.Rock", self.rock);
        self.ripple_radius = section.read_ares_nullable_int("Ripple.Radius", self.ripple_radius);
        self.ripple_radius = section.read_ares_nullable_int("IonCannon.Ripple", self.ripple_radius);
        for (field, value) in [
            (&mut self.blast, blast),
            (&mut self.beam, beam),
            (&mut self.warhead, warhead),
        ] {
            if value.is_some() {
                *field = value;
            }
        }
        self.damage = section.read_ares_nullable_int("IonCannon.Damage", self.damage);
    }
    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }
    pub(crate) fn rock(&self) -> bool {
        self.rock
    }
    /// applyRipples 10051AE0 raises a blast for `IonCannon=yes` or a
    /// positive `Ripple.Radius`.
    pub(crate) fn raises_blast(&self) -> bool {
        self.enabled || self.ripple_radius.unwrap_or(0) > 0
    }
    /// IonBlastClass_Update_Duration (100541B0): the blast lives while its
    /// Lifetime is below `Ripple.Radius + 1` (default 79 for an ion cannon,
    /// else 0), at most 79.
    pub(crate) fn blast_lifetime(&self) -> i32 {
        let radius = self
            .ripple_radius
            .unwrap_or(if self.enabled { 79 } else { 0 });
        radius.wrapping_add(1).min(79)
    }
    /// The Nullables' value, else the given `[General]`/`[CombatDamage]`
    /// default.
    pub(crate) fn blast<'a>(&'a self, rules_default: &'a str) -> Option<&'a str> {
        nullable_name(&self.blast, rules_default)
    }
    pub(crate) fn beam<'a>(&'a self, rules_default: &'a str) -> Option<&'a str> {
        nullable_name(&self.beam, rules_default)
    }
    pub(crate) fn warhead<'a>(&'a self, rules_default: &'a str) -> Option<&'a str> {
        nullable_name(&self.warhead, rules_default)
    }
    pub(crate) fn damage(&self, rules_default: i32) -> i32 {
        self.damage.unwrap_or(rules_default)
    }
}

#[cfg(feature = "ares")]
fn nullable_name<'a>(value: &'a Option<Option<String>>, rules_default: &'a str) -> Option<&'a str> {
    match value {
        Some(name) => name.as_deref(),
        None => Some(rules_default),
    }
}

/// Ares Promotable<int>::Read (TemplateDef.h, noinline): the format with ""
/// (a trailing dot stripped) sets all three ranks when it parses, then the
/// Rookie, Veteran and Elite keys each override one; failures retain.
#[cfg(feature = "ares")]
fn read_promotable_int(
    section: &IniSection,
    [common, rookie]: [&str; 2],
    [veteran, elite]: [&str; 2],
    current: [i32; 3],
) -> [i32; 3] {
    let mut value = section
        .read_ares_nullable_int(common, None)
        .map_or(current, |all| [all; 3]);
    for (slot, key) in [rookie, veteran, elite].into_iter().enumerate() {
        value[slot] = section.read_ares_int(key, value[slot]);
    }
    value
}

/// Shared descriptor: WarheadExt+88 (constructor10051890) and
/// TechnoTypeExt+290 (constructor1003CA84). Both call ReadINI10058A50;
/// TechnoType does so at1003E8D4. Instances live in TechnoExt+20.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AttachEffectRules {
    duration: i32,
    cumulative: bool,
    animation: Option<String>,
    anim_reset_on_reapply: bool,
    temporal_hides_anim: bool,
    penetrates_iron_curtain: bool,
    force_decloak: bool,
    discard_on_entry: bool,
    firepower: NativeF64Bits,
    armor: NativeF64Bits,
    rof: NativeF64Bits,
    speed: NativeF64Bits,
    cloakable: bool,
    delay: i32,
    initial_delay: i32,
}
impl Default for AttachEffectRules {
    fn default() -> Self {
        Self {
            duration: 0,
            cumulative: false,
            animation: None,
            anim_reset_on_reapply: false,
            temporal_hides_anim: false,
            penetrates_iron_curtain: false,
            force_decloak: false,
            discard_on_entry: false,
            firepower: NativeF64Bits::ONE,
            armor: NativeF64Bits::ONE,
            rof: NativeF64Bits::ONE,
            speed: NativeF64Bits::ONE,
            cloakable: false,
            delay: 0,
            initial_delay: 0,
        }
    }
}
impl AttachEffectRules {
    pub(crate) fn duration(&self) -> i32 {
        self.duration
    }
    pub(crate) fn animation(&self) -> Option<&str> {
        self.animation.as_deref()
    }
    pub(crate) fn cumulative(&self) -> bool {
        self.cumulative
    }
    pub(crate) fn anim_reset_on_reapply(&self) -> bool {
        self.anim_reset_on_reapply
    }
    pub(crate) fn discard_on_entry(&self) -> bool {
        self.discard_on_entry
    }
    pub(crate) fn force_decloak(&self) -> bool {
        self.force_decloak
    }
    pub(crate) fn penetrates_iron_curtain(&self) -> bool {
        self.penetrates_iron_curtain
    }
    pub(crate) fn speed_multiplier(&self) -> NativeF64Bits {
        self.speed
    }
    pub(crate) fn firepower_multiplier(&self) -> NativeF64Bits {
        self.firepower
    }
    pub(crate) fn armor_multiplier(&self) -> NativeF64Bits {
        self.armor
    }
    pub(crate) fn rof_multiplier(&self) -> NativeF64Bits {
        self.rof
    }
    pub(crate) fn cloakable(&self) -> bool {
        self.cloakable
    }
    pub(crate) fn delay(&self) -> i32 {
        self.delay
    }
    pub(crate) fn initial_delay(&self) -> i32 {
        self.initial_delay
    }
    /// Warhead and self effects share the stat/cloak aggregates. Temporal
    /// animation visibility remains a separate mechanism.
    pub(crate) fn visual_combat_path(&self) -> bool {
        !self.temporal_hides_anim
    }
    #[cfg(feature = "ares")]
    pub(crate) fn read_pass(&mut self, section: &IniSection, animation: Option<Option<String>>) {
        let _ares = crate::rules::ares_switch::ares_reads();
        // Exact 10058A59..10058B3A read order. Missing keys retain the live
        // constructor/prior-layer value; existing scalar readers own parsing.
        self.duration = section.read_ares_int("AttachEffect.Duration", self.duration);
        self.cumulative = section.read_ares_bool("AttachEffect.Cumulative", self.cumulative);
        if let Some(animation) = animation {
            self.animation = animation;
        }
        self.anim_reset_on_reapply = section.read_ares_bool(
            "AttachEffect.AnimResetOnReapply",
            self.anim_reset_on_reapply,
        );
        self.temporal_hides_anim =
            section.read_ares_bool("AttachEffect.TemporalHidesAnim", self.temporal_hides_anim);
        self.penetrates_iron_curtain = section.read_ares_bool(
            "AttachEffect.PenetratesIronCurtain",
            self.penetrates_iron_curtain,
        );
        self.force_decloak =
            section.read_ares_bool("AttachEffect.ForceDecloak", self.force_decloak);
        self.discard_on_entry =
            section.read_ares_bool("AttachEffect.DiscardOnEntry", self.discard_on_entry);
        self.firepower =
            section.read_ares_double_bits("AttachEffect.FirepowerMultiplier", self.firepower);
        self.armor = section.read_ares_double_bits("AttachEffect.ArmorMultiplier", self.armor);
        self.rof = section.read_ares_double_bits("AttachEffect.ROFMultiplier", self.rof);
        self.speed = section.read_ares_double_bits("AttachEffect.SpeedMultiplier", self.speed);
        self.cloakable = section.read_ares_bool("AttachEffect.Cloakable", self.cloakable);
        self.delay = section.read_ares_int("AttachEffect.Delay", self.delay);
        self.initial_delay = section.read_ares_int("AttachEffect.InitialDelay", self.initial_delay);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{
        ini_parser::IniFile,
        native_processing::{RulesLayerKind, RulesLayerStack},
        ruleset::RuleSet,
    };

    #[test]
    fn self_attach_effect_uses_retained_type_reader_and_build_gate() {
        let root = IniFile::from_str(
            "[VehicleTypes]\n0=SELF_TEST\n[Animations]\n0=GLOW\n\
            [SELF_TEST]\nStrength=100\nAttachEffect.Duration=3\nAttachEffect.Delay=2\n\
            AttachEffect.InitialDelay=2\nAttachEffect.Cloakable=yes\nAttachEffect.Animation=GLOW\n",
        );
        let read = |extra: &str| {
            let mut layers = RulesLayerStack::new(root.clone());
            layers.push(RulesLayerKind::Scenario, IniFile::from_str(extra));
            RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap()
        };
        let rules = read(
            "[SELF_TEST]\nAttachEffect.Duration=junk\nAttachEffect.InitialDelay=0\nCloakable.Allowed=no\n",
        );
        let kind = rules.object("SELF_TEST").unwrap();
        if crate::rules::ARES_BUILD {
            let effect = kind.self_attach_effect().unwrap();
            assert_eq!(
                (effect.duration(), effect.delay(), effect.initial_delay()),
                (3, 2, 0)
            );
            assert!(effect.cloakable());
            assert_eq!(effect.animation(), Some("GLOW"));
            assert!(!kind.cloak_allowed());
            assert_ne!(
                rules.simulation_config_hash(),
                read("[SELF_TEST]\nAttachEffect.Duration=5\n").simulation_config_hash()
            );
        } else {
            assert!(kind.self_attach_effect().is_none());
            assert!(kind.cloak_allowed());
        }
    }

    #[test]
    fn attach_effect_production_layers_retain_registry_references_and_feature_gate() {
        // All fifteen descriptor keys go through the retained production
        // processor, including invalid/missing later reads and Anim Find.
        let root = IniFile::from_str(
            "[Warheads]\n0=ATTACH_WH\n1=DEFAULT_WH\n\
             [Animations]\n0=AE_VISUAL\n\
             [ATTACH_WH]\nEMP.Duration=7\nAffectsOwner=yes\n\
             AttachEffect.Duration=5\nAttachEffect.Cumulative=yes\n\
             AttachEffect.Animation=ae_visual\nAttachEffect.AnimResetOnReapply=yes\n\
             AttachEffect.TemporalHidesAnim=yes\nAttachEffect.PenetratesIronCurtain=yes\n\
             AttachEffect.ForceDecloak=yes\nAttachEffect.DiscardOnEntry=yes\n\
             AttachEffect.FirepowerMultiplier=2\nAttachEffect.ArmorMultiplier=0.5\n\
             AttachEffect.ROFMultiplier=3\nAttachEffect.SpeedMultiplier=4\n\
             AttachEffect.Cloakable=yes\nAttachEffect.Delay=9\nAttachEffect.InitialDelay=11\n\
             [DEFAULT_WH]\nCellSpread=0\n",
        );
        let scenario = "[ATTACH_WH]\nAttachEffect.Duration=junk\n\
                        AttachEffect.Animation=UNREGISTERED\nAttachEffect.SpeedMultiplier=\n";
        let read = |source: IniFile, scenario: &str| {
            let mut layers = RulesLayerStack::new(source);
            layers.push(RulesLayerKind::Scenario, IniFile::from_str(scenario));
            RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap()
        };
        let rules = read(root.clone(), scenario);
        let changed = read(root.clone(), "[ATTACH_WH]\nAttachEffect.Duration=6\n");
        let cleared = read(root, "[ATTACH_WH]\nAttachEffect.Animation=<none>\n");
        if crate::rules::ARES_BUILD {
            let wh = rules.warhead("ATTACH_WH").unwrap().ares_rules().unwrap();
            assert_eq!(wh.duration(), 7, "existing EMP state remains independent");
            assert!(wh.can_affect_house(true, true, true, false));
            let ae = wh.attach_effect();
            assert_eq!(ae.duration, 5);
            assert!(ae.cumulative);
            assert_eq!(ae.animation(), Some("AE_VISUAL"));
            assert!(ae.anim_reset_on_reapply);
            assert!(ae.temporal_hides_anim);
            assert!(ae.penetrates_iron_curtain);
            assert!(ae.force_decloak);
            assert!(ae.discard_on_entry);
            assert_eq!(ae.firepower, NativeF64Bits::from_bits(2.0_f64.to_bits()));
            assert_eq!(ae.armor, NativeF64Bits::from_bits(0.5_f64.to_bits()));
            assert_eq!(ae.rof, NativeF64Bits::from_bits(3.0_f64.to_bits()));
            assert_eq!(ae.speed, NativeF64Bits::from_bits(4.0_f64.to_bits()));
            assert!(ae.cloakable);
            assert_eq!((ae.delay, ae.initial_delay), (9, 11));
            assert!(!ae.visual_combat_path());
            assert_eq!(
                rules
                    .warhead("DEFAULT_WH")
                    .unwrap()
                    .ares_rules()
                    .unwrap()
                    .attach_effect(),
                &AttachEffectRules::default(),
                "constructor defaults do not come from retail tuning"
            );
            assert_eq!(
                cleared
                    .warhead("ATTACH_WH")
                    .unwrap()
                    .ares_rules()
                    .unwrap()
                    .attach_effect()
                    .animation(),
                None,
                "successful native none clears the retained reference"
            );
            assert_ne!(
                rules.simulation_config_hash(),
                changed.simulation_config_hash()
            );
            assert_ne!(
                rules.simulation_config_hash(),
                cleared.simulation_config_hash()
            );
        } else {
            for candidate in [&rules, &changed, &cleared] {
                assert!(
                    candidate
                        .warhead("ATTACH_WH")
                        .unwrap()
                        .ares_rules()
                        .is_none()
                );
            }
            // The whole configuration hash deliberately includes raw source
            // identity, even for unconsumed keys (RuleSet::source_ini_hash).
            // Ignoring extension state does not erase different INI inputs.
            assert_ne!(rules.source_ini_hash(), changed.source_ini_hash());
            assert_ne!(rules.source_ini_hash(), cleared.source_ini_hash());
            assert_ne!(
                rules.simulation_config_hash(),
                changed.simulation_config_hash()
            );
            assert_ne!(
                rules.simulation_config_hash(),
                cleared.simulation_config_hash()
            );
        }
    }

    /// WarheadTypeExt ReadINI 10051D80's combat group through the retained
    /// production processor: constructor defaults, Promotable culling keys,
    /// Nullable retention, the TechnoType list, the Voc overrides and the
    /// two RulesExt keys of LoadBeforeTypeData 1002AB60.
    #[test]
    #[cfg(feature = "ares")]
    fn combat_keys_use_the_ares_readers_and_constructor_defaults() {
        use crate::rules::object_type::ObjectCategory;
        let root = IniFile::from_str(
            "[Warheads]\n0=CULL\n1=PLAIN\n[InfantryTypes]\n0=GI\n[VehicleTypes]\n0=MTNK\n\
             [General]\nDamageAirConsiderBridges=yes\n[AudioVisual]\nIronCurtainFlash=no\n\
             [CULL]\nCulling.BelowHealth=40\nCulling.EliteBelowHealth=-2\nCulling.VeteranChance=30\n\
             RelativeDamage=yes\nRelativeDamage.Vehicles=-50\nRelativeDamage.Terrain=7\n\
             Damage.Deployed=50%\nMalicious=no\nPreventScatter=yes\nBridgeAbsoluteDestroyer=yes\n\
             CellSpread.MaxAffect=1\nDamageAirThreshold=300\nUnitLost.Suppress=yes\n\
             DeathWeapon.SuppressInfantry=yes\nDeathWeapon.Suppress=MTNK,NOPE\n\
             Flash.Duration=12\nAllowZeroDamage=yes\nIronCurtain.Flash=yes\n\
             DieSound.Override=Boom\nVoiceDie.Override=none\n\
             Temporal.HealthFactor=0.5\nTemporal.WarpAway=none\n\
             [PLAIN]\nCellSpread=0\n",
        );
        let mut layers = RulesLayerStack::new(root);
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[CULL]\nBridgeAbsoluteDestroyer=junk\nDamageAirThreshold=\n"),
        );
        let processed = layers.process().unwrap();
        let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
        rules.bind_type_sound_references(
            processed.ini(),
            &crate::rules::sound_ini::SoundRegistry::from_ini(&IniFile::from_str(
                "[SoundList]\n0=Boom\n[Boom]\nSounds=boom\n",
            )),
        );
        let combat = |name: &str| rules.warhead(name).unwrap().ares_combat().unwrap();
        let cull = combat("CULL");
        assert_eq!(cull.culling(0), (40, -1));
        assert_eq!(cull.culling(1), (40, 30));
        assert_eq!(cull.culling(2), (-2, -1));
        assert_eq!(
            cull.relative_damage(Some(RelativeDamageKind::Vehicles), 300, Some(1000)),
            Some(150)
        );
        assert_eq!(
            cull.relative_damage(Some(RelativeDamageKind::Terrain), 300, Some(1000)),
            Some(70)
        );
        assert_eq!(
            cull.deployed_damage(),
            NativeF64Bits::from_bits(0.5_f64.to_bits())
        );
        assert!(!cull.malicious() && cull.prevent_scatter());
        assert!(cull.bridge_absolute_destroyer(false), "junk retains yes");
        assert_eq!(
            (cull.cell_spread_max_affect(), cull.damage_air_threshold()),
            (1, 300)
        );
        assert!(cull.unit_lost_suppress() && cull.allow_zero_damage());
        assert!(cull.suppresses_death_weapon(ObjectCategory::Infantry, "GI"));
        assert!(cull.suppresses_death_weapon(ObjectCategory::Vehicle, "MTNK"));
        assert!(!cull.suppresses_death_weapon(ObjectCategory::Aircraft, "NOPE"));
        assert_eq!(cull.flash_duration(), 12);
        assert_eq!(cull.iron_curtain_flash(), Some(true));
        assert_eq!(cull.die_sound_override(), Some(Some("Boom")));
        assert_eq!(cull.voice_die_override(), Some(None));
        assert_eq!(
            cull.temporal_health_factor(),
            NativeF64Bits::from_bits(0.5_f64.to_bits())
        );
        assert_eq!(cull.temporal_warp_away("WARPAWAY"), None);
        assert_eq!(
            combat("PLAIN").temporal_warp_away("WARPAWAY"),
            Some("WARPAWAY")
        );
        let plain = combat("PLAIN");
        assert_eq!(plain, &WarheadCombatRules::default());
        assert_eq!((plain.culling(0), plain.culling(2)), ((0, -1), (-1, -1)));
        assert!(plain.malicious() && plain.bridge_absolute_destroyer(true));
        assert_eq!(plain.relative_damage(None, 1, Some(1)), None);
        assert_eq!(plain.die_sound_override(), None);
        assert!(rules.ares_warhead_globals().damage_air_consider_bridges());
        assert!(!rules.ares_warhead_globals().iron_curtain_flash());
    }

    /// `InfDeathAnim` is the only field naming this animation, so nothing else
    /// puts it in the AnimClass construction-root closure. Mental Omega's GBION
    /// warheads rely on exactly that shape (`InfDeathAnim=FREEZE` etc. with no
    /// `InfDeath=`), and an unrooted type makes the constructor refuse with
    /// `animation type [X] has no bound SHP frame count`, so the kill plays no
    /// death animation at all.
    ///
    /// The assertion reads `ares_combat()`, which only exists in Ares builds.
    #[cfg(feature = "ares")]
    #[test]
    fn inf_death_anim_joins_the_anim_class_construction_roots() {
        let root = IniFile::from_str(
            "[Warheads]\n0=DEATH_WH\n\
             [Animations]\n0=MELTDIEE\n\
             [DEATH_WH]\nCellSpread=1\nInfDeathAnim=MELTDIEE\n",
        );
        let rules = RuleSet::from_processed_rules(&RulesLayerStack::new(root).process().unwrap())
            .unwrap();
        assert_eq!(
            rules
                .warhead("DEATH_WH")
                .and_then(|warhead| warhead.ares_combat())
                .and_then(|combat| combat.inf_death_anim()),
            Some("MELTDIEE")
        );
        assert!(
            crate::rules::effect_asset_catalog::anim_class_roots(&rules)
                .contains(&"MELTDIEE".to_string()),
            "InfDeathAnim must bind its SHP frames or the death anim never builds"
        );
    }
}
