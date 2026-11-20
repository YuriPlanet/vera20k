//! Phobos Shields, retained by the Rules registry.
//!
//! Native identity: Phobos `ShieldTypeClass`
//! (`src/New/Type/ShieldTypeClass.{h,cpp}`), an `Enumerable<ShieldTypeClass>`
//! whose main section is `ShieldTypes` and whose entries default to an
//! inactive `Strength=0`. `ShieldTypeClass::LoadFromINI` reads each type's own
//! section; `TechnoTypeExt::ShieldType` (`ShieldType=`) binds a techno, and
//! `TechnoExt` constructs a `ShieldClass` only when the bound type's strength is
//! above zero (`Hooks.cpp:297..301`).
//!
//! The catalog is read by the ordered Rules Process owner, exactly as the
//! `[RadiationTypes]` catalog is, so a later layer replaces or retains each
//! field independently. `RuleSet` publishes an immutable projection and the
//! simulation resolves shields through [`crate::rules::ruleset::RuleSet::shield_type`].
//! Damage absorption itself lives in `sim::shield`, at the
//! `TechnoClass::ReceiveDamage @ 0x00701900` entry
//! (installed hook `0x701900+0x6`, `src/Ext/Techno/Hooks.ReceiveDamage.cpp`).
//!
//! Read but not consumed here: `Pips*` (drawn by `app/presentation/ui_overlays`).
//! `AllowTransfer`/`AllowTransfer.Convert` are consumed by
//! `sim::shield`'s `ConvertCheck` on the conversion and deploy paths. `IdleAnim*` is consumed by
//! `sim::shield` and `HitFlash*` feeds `sim::combat::CombatLightRequest`
//! through the same owner. The warhead-side shield keys live in
//! [`ShieldWarheadRules`]; the simulation owner `sim::shield` consumes them.

use crate::rules::attach_effect::AttachedAnimFlag;
use crate::rules::ini_parser::IniFile;
use crate::rules::ini_value::truncate_native_bytes;

/// `Enumerable` stores `Name[32]` with `_TRUNCATE`.
const NAME_PAYLOAD_BYTES: usize = 0x1F;

/// Phobos `AffectedHouse` flags (`Utilities/Enum.h:127..141`), the target of
/// `Valueable<AffectedHouse>::Read` (`Utilities/TemplateDef.h:824..873`): the
/// value starts at `None` and ORs one flag per comma-separated token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AffectedHouse(u8);

impl AffectedHouse {
    pub const NONE: Self = Self(0x0);
    pub const OWNER: Self = Self(0x1);
    pub const ALLIES: Self = Self(0x2);
    pub const ENEMIES: Self = Self(0x4);
    pub const NEUTRAL: Self = Self(0x8);
    /// `Team` = Owner | Allies (`Utilities/Enum.h`, the `RevealHouses` default).
    pub const TEAM: Self = Self(Self::OWNER.0 | Self::ALLIES.0);
    pub const ALL: Self = Self(Self::OWNER.0 | Self::ALLIES.0 | Self::ENEMIES.0 | Self::NEUTRAL.0);

    pub fn bits(self) -> u8 {
        self.0
    }

    /// The `flags & AffectedHouse::X != AffectedHouse::None` test.
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// `Read<AffectedHouse>`: every token must match a known spelling, or the
    /// whole read fails and the caller retains its current value.
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        let mut parsed = Self::NONE;
        for part in raw.split(',') {
            let token = part.trim_matches(|c| matches!(c, ' ' | '\t' | '\r'));
            let flag = match token.to_ascii_lowercase().as_str() {
                "owner" | "self" => Self::OWNER,
                "allies" | "ally" => Self::ALLIES,
                "enemies" | "enemy" => Self::ENEMIES,
                "neutral" => Self::NEUTRAL,
                // Team = Owner | Allies, Others = NotOwner.
                "team" => Self::TEAM,
                "others" => Self(Self::ALLIES.0 | Self::ENEMIES.0),
                "all" => Self::ALL,
                "none" => Self::NONE,
                _ => return None,
            };
            parsed = Self(parsed.0 | flag.0);
        }
        Some(parsed)
    }
}

/// `ShieldTypeClass`'s constructor default (`Tint_VisibleToHouses { AffectedHouse::All }`).
impl Default for AffectedHouse {
    fn default() -> Self {
        Self::ALL
    }
}

/// `Valueable<AffectedHouse>` read: absent or rejected text keeps `current`.
pub(crate) fn read_affected_house(
    section: &crate::rules::ini_parser::IniSection,
    key: &str,
    current: AffectedHouse,
) -> AffectedHouse {
    section.read_string_with(key, 2048, current, |current, raw| {
        AffectedHouse::parse(raw).unwrap_or(current)
    })
}

/// `EnumFunctions::CanTargetHouse` (`EnumFunctions.cpp:5..16`): the shared
/// owner/neutral/alliance ladder. `is_owner` is source==target, `is_neutral`
/// the target house's `MultiplayPassive`, `is_allied` the source/target
/// alliance. Pure decision logic so the sim combat owner and the radar's
/// `RadarInvisibleToHouse` test share one port instead of copying the ladder.
pub fn can_target_house_ladder(
    flags: AffectedHouse,
    is_owner: bool,
    is_neutral: bool,
    is_allied: bool,
) -> bool {
    if flags == AffectedHouse::ALL {
        return true;
    }
    if is_owner {
        return flags.contains(AffectedHouse::OWNER);
    }
    if is_neutral && flags.contains(AffectedHouse::NEUTRAL) {
        return true;
    }
    if is_allied {
        flags.contains(AffectedHouse::ALLIES)
    } else {
        flags.contains(AffectedHouse::ENEMIES)
    }
}

/// `ShieldTypeClass`'s constructor value for both `IdleAnim` actions.
fn attached_anim_hides() -> AttachedAnimFlag {
    AttachedAnimFlag::Hides
}

/// Phobos `Damageable<AnimTypeClass*>` (`Utilities/Template.h:409-474`): the
/// base animation plus nullable `ConditionYellow`/`ConditionRed` overrides.
/// The base flag's trailing dot is stripped (`Utilities/TemplateDef.h:2227-2245`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ShieldIdleAnimSet {
    /// `<Key>`: the base animation as the `Valueable<AnimTypeClass*>` read
    /// retained it; `none` spellings clear it, `<default>` retains.
    #[serde(default)]
    base: Option<String>,
    /// `<Key>.ConditionYellow=`: `Nullable<AnimTypeClass*>::Read<false>`
    /// (`Utilities/TemplateDef.h:1834..1849`), whose `allowNone=false` makes
    /// `none`/`<none>` a reset rather than a present null.
    #[serde(default)]
    condition_yellow: Option<String>,
    /// `<Key>.ConditionRed=`.
    #[serde(default)]
    condition_red: Option<String>,
}

impl ShieldIdleAnimSet {
    pub(crate) fn apply(&mut self, section: &crate::rules::ini_parser::IniSection, key: &str) {
        self.base = read_anim_value(section, key, self.base.take());
        self.condition_yellow = read_anim_band(
            section,
            &format!("{key}.ConditionYellow"),
            self.condition_yellow.take(),
        );
        self.condition_red = read_anim_band(
            section,
            &format!("{key}.ConditionRed"),
            self.condition_red.take(),
        );
    }

    /// `ConditionYellow || ConditionRed` set.
    pub(crate) fn is_damaged_value_set(&self) -> bool {
        self.condition_yellow.is_some() || self.condition_red.is_some()
    }

    /// `BaseValue || ConditionYellow.isset() || ConditionRed.isset()`.
    pub(crate) fn isset(&self) -> bool {
        self.base.is_some() || self.is_damaged_value_set()
    }

    /// `Damageable::Get(ratio, conditionYellow, conditionRed)`.
    pub(crate) fn get(&self, ratio: f64, yellow: f64, red: f64) -> Option<&str> {
        if self.condition_red.is_some() && ratio <= red {
            return self.condition_red.as_deref();
        }
        if self.condition_yellow.is_some() && ratio <= yellow {
            return self.condition_yellow.as_deref();
        }
        self.base.as_deref()
    }

    /// The unset base, for the no-damaged-values arm of `CreateAnim`.
    pub(crate) fn base(&self) -> Option<&str> {
        self.base.as_deref()
    }
}

/// `Valueable<AnimTypeClass*>::Read` (`Utilities/TemplateDef.h:1793..1797` ->
/// `:53..78`): `none`/`<none>` are `INIClass::IsBlank` (0x40F4D0 compares
/// those two exact sentinels), so a blank assigns null; an unknown name or
/// `<default>` fails the parse and retains the current value. The default
/// build has no shield idle animations, so it leaves the value unset.
fn read_anim_value(
    section: &crate::rules::ini_parser::IniSection,
    key: &str,
    current: Option<String>,
) -> Option<String> {
    {
        section.read_string_with(key, 2048, current, |current, raw| {
            if raw.eq_ignore_ascii_case("<default>") {
                current
            } else if is_none_name(raw) {
                None
            } else {
                Some(raw.to_owned())
            }
        })
    }
}

/// `Nullable<AnimTypeClass*>::Read<false, false>` (`Utilities/TemplateDef.h`
/// `:1834..1849`): `<default>` and the `none`/`<none>` spellings that
/// `INIClass::IsBlank` (0x40F4D0) accepts reset the band; any other name
/// becomes the value.
fn read_anim_band(
    section: &crate::rules::ini_parser::IniSection,
    key: &str,
    current: Option<String>,
) -> Option<String> {
    {
        section.read_string_with(key, 2048, current, |_, raw| {
            if raw.eq_ignore_ascii_case("<default>") || is_none_name(raw) {
                None
            } else {
                Some(raw.to_owned())
            }
        })
    }
}

/// `ShieldTypeClass`'s constructor values. Every field keeps the current value
/// when its key is absent, so one `apply_pass` may replace or retain each value.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct ShieldType {
    name: String,
    strength: i32,
    initial_strength: Option<i32>,
    condition_yellow: Option<f64>,
    condition_red: Option<f64>,
    /// Phobos `IdleAnim=` (`ShieldTypeClass.h:36`, `Damageable<AnimTypeClass*>`,
    /// constructor unset `ShieldTypeClass.cpp:94`): the shield's idle
    /// animation, with per-condition variants.
    #[serde(default)]
    idle_anim: ShieldIdleAnimSet,
    /// `IdleAnimDamaged=` (`ShieldTypeClass.h:37`, unset `:95`): the idle
    /// animation while the techno is at or below `[General] ConditionYellow`.
    #[serde(default)]
    idle_anim_damaged: ShieldIdleAnimSet,
    /// `IdleAnim.OfflineAction=` (`ShieldTypeClass.cpp:56`,
    /// `Valueable<AttachedAnimFlag>` constructor `Hides` `:92`).
    #[serde(default = "attached_anim_hides")]
    idle_anim_offline_action: AttachedAnimFlag,
    /// `IdleAnim.TemporalAction=` (`:57`, constructor `Hides` `:93`).
    #[serde(default = "attached_anim_hides")]
    idle_anim_temporal_action: AttachedAnimFlag,
    armor: String,
    inherit_armor_from_techno: bool,
    inherit_armor_allowed: Vec<String>,
    inherit_armor_disallowed: Vec<String>,
    apply_armor_mult: Option<bool>,
    powered: bool,
    respawn: f64,
    respawn_rate: i32,
    respawn_restart_in_combat: bool,
    respawn_restart_in_combat_delay: i32,
    respawn_anim: Vec<String>,
    respawn_weapon: Option<String>,
    self_healing: f64,
    self_healing_rate: i32,
    self_healing_restart_in_combat: bool,
    self_healing_restart_in_combat_delay: i32,
    self_healing_enabled_by: Vec<String>,
    absorb_over_damage: bool,
    immune_to_berserk: bool,
    immune_to_crit: bool,
    bracket_delta: i32,
    break_anim: Vec<String>,
    hit_anim: Vec<String>,
    hit_flash: bool,
    hit_flash_fixed_size: Option<i32>,
    hit_flash_red: bool,
    hit_flash_green: bool,
    hit_flash_blue: bool,
    hit_flash_black: bool,
    break_weapon: Option<String>,
    absorb_percent: f64,
    pass_percent: f64,
    received_damage_minimum: i32,
    received_damage_maximum: i32,
    pips: [i32; 3],
    pips_building: [i32; 3],
    /// `AllowTransfer` (`ShieldTypeClass.h:53`, `Nullable<bool>`, unset
    /// `:108`): whether the shield survives onto a converted object. The
    /// reading falls back to `Attached`, so an unset key keeps a
    /// warhead-attached shield and drops a type-bound one.
    #[serde(default)]
    allow_transfer: Option<bool>,
    /// `AllowTransfer.Convert` (`:54`, unset `:109`): the conversion override,
    /// which falls back to `AllowTransfer.Get(Attached)`.
    #[serde(default)]
    allow_transfer_convert: Option<bool>,
    pips_background: Option<String>,
    pips_building_empty: Option<i32>,
    pips_hide_if_no_strength: bool,
    tint_color: Option<(u8, u8, u8)>,
    tint_intensity: f64,
    tint_visible_to_houses: AffectedHouse,
}

impl ShieldType {
    fn named(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            armor: "none".to_owned(),
            // ShieldTypeClass's constructor:
            //   Respawn/SelfHealing 0, rates 0, RestartInCombat true, delays 0,
            //   HitFlash false, channels true, AbsorbPercent 1.0, PassPercent 0,
            //   ReceivedDamage min/max INT32_MIN/INT32_MAX.
            respawn_restart_in_combat: true,
            self_healing_restart_in_combat: true,
            hit_flash_red: true,
            hit_flash_green: true,
            hit_flash_blue: true,
            absorb_percent: 1.0,
            received_damage_minimum: i32::MIN,
            received_damage_maximum: i32::MAX,
            pips: [-1, -1, -1],
            pips_building: [-1, -1, -1],
            idle_anim_offline_action: AttachedAnimFlag::Hides,
            idle_anim_temporal_action: AttachedAnimFlag::Hides,
            ..Self::default()
        }
    }

    /// `InitialStrength.Get(Strength)`: the constructor's nullable default.
    pub(crate) fn initial_strength(&self) -> i32 {
        self.initial_strength.unwrap_or(self.strength)
    }

    pub fn strength(&self) -> i32 {
        self.strength
    }

    pub fn condition_yellow(&self, global: f64) -> f64 {
        self.condition_yellow.unwrap_or(global)
    }

    pub fn condition_red(&self, global: f64) -> f64 {
        self.condition_red.unwrap_or(global)
    }

    pub(crate) fn idle_anim(&self) -> &ShieldIdleAnimSet {
        &self.idle_anim
    }

    pub(crate) fn idle_anim_damaged(&self) -> &ShieldIdleAnimSet {
        &self.idle_anim_damaged
    }

    pub(crate) fn idle_anim_offline_action(&self) -> AttachedAnimFlag {
        self.idle_anim_offline_action
    }

    pub(crate) fn idle_anim_temporal_action(&self) -> AttachedAnimFlag {
        self.idle_anim_temporal_action
    }

    /// `ShieldTypeClass::HitFlash` (`ShieldTypeClass.h:40`, member
    /// initialiser `false` `ShieldTypeClass.h:98`): the absorbed-hit
    /// spotlight gate.
    pub(crate) fn hit_flash(&self) -> bool {
        self.hit_flash
    }

    /// `HitFlash.FixedSize` (`Nullable<int>`, `ShieldTypeClass.h:41`,
    /// `:99`): the flash strength, defaulting to twice the clamped shield
    /// damage.
    pub(crate) fn hit_flash_fixed_size(&self) -> Option<i32> {
        self.hit_flash_fixed_size
    }

    /// `HitFlash.Red` (`ShieldTypeClass.h:42`, default `true` `:100`).
    pub(crate) fn hit_flash_red(&self) -> bool {
        self.hit_flash_red
    }

    /// `HitFlash.Green` (`ShieldTypeClass.h:43`, default `true` `:101`).
    pub(crate) fn hit_flash_green(&self) -> bool {
        self.hit_flash_green
    }

    /// `HitFlash.Blue` (`ShieldTypeClass.h:44`, default `true` `:102`).
    pub(crate) fn hit_flash_blue(&self) -> bool {
        self.hit_flash_blue
    }

    /// `HitFlash.Black` (`ShieldTypeClass.h:45`, default `false` `:103`).
    pub(crate) fn hit_flash_black(&self) -> bool {
        self.hit_flash_black
    }

    pub fn has_tint(&self) -> bool {
        self.tint_color.is_some() || self.tint_intensity != 0.0
    }

    pub(crate) fn armor(&self) -> &str {
        &self.armor
    }

    pub(crate) fn inherit_armor_from_techno(&self) -> bool {
        self.inherit_armor_from_techno
    }

    pub(crate) fn inherit_armor_allowed(&self) -> &[String] {
        &self.inherit_armor_allowed
    }

    pub(crate) fn inherit_armor_disallowed(&self) -> &[String] {
        &self.inherit_armor_disallowed
    }

    pub(crate) fn apply_armor_mult(&self, global: bool) -> bool {
        self.apply_armor_mult.unwrap_or(global)
    }

    pub(crate) fn powered(&self) -> bool {
        self.powered
    }

    /// `AllowTransfer` (`ShieldTypeClass.h:53`).
    pub(crate) fn allow_transfer(&self) -> Option<bool> {
        self.allow_transfer
    }

    /// `AllowTransfer.Convert` (`ShieldTypeClass.h:54`).
    pub(crate) fn allow_transfer_convert(&self) -> Option<bool> {
        self.allow_transfer_convert
    }

    /// `ShieldClass::ConvertCheck`'s `allowTransfer`
    /// (`ShieldClass.cpp:612-613`): the conversion override falls back to
    /// `AllowTransfer.Get(Attached)`, and an unset `AllowTransfer` falls back
    /// to `Attached` itself.
    pub(crate) fn conversion_transfer(&self, attached: bool) -> bool {
        self.allow_transfer_convert
            .unwrap_or(self.allow_transfer.unwrap_or(attached))
    }

    pub(crate) fn respawn(&self) -> f64 {
        self.respawn
    }

    pub(crate) fn respawn_rate(&self) -> i32 {
        self.respawn_rate
    }

    pub(crate) fn respawn_restart_in_combat(&self) -> bool {
        self.respawn_restart_in_combat
    }

    pub(crate) fn respawn_restart_in_combat_delay(&self) -> i32 {
        self.respawn_restart_in_combat_delay
    }

    pub(crate) fn respawn_anim(&self) -> &[String] {
        &self.respawn_anim
    }

    pub(crate) fn respawn_weapon(&self) -> Option<&str> {
        self.respawn_weapon.as_deref()
    }

    /// `SelfHealing.EnabledBy` (`ShieldTypeClass.cpp:49`): an empty list means
    /// self-healing is enabled for every owner.
    pub(crate) fn self_healing_enabled_by(&self) -> &[String] {
        &self.self_healing_enabled_by
    }

    /// `SelfHealing` (`ShieldTypeClass.cpp:41`).
    pub(crate) fn self_healing(&self) -> f64 {
        self.self_healing
    }

    /// `SelfHealing` (`ShieldTypeClass.cpp:41`).
    pub(crate) fn self_healing_rate(&self) -> i32 {
        self.self_healing_rate
    }

    pub(crate) fn self_healing_restart_in_combat(&self) -> bool {
        self.self_healing_restart_in_combat
    }

    pub(crate) fn self_healing_restart_in_combat_delay(&self) -> i32 {
        self.self_healing_restart_in_combat_delay
    }

    pub(crate) fn absorb_over_damage(&self) -> bool {
        self.absorb_over_damage
    }

    pub(crate) fn immune_to_berserk(&self) -> bool {
        self.immune_to_berserk
    }

    /// Phobos `ShieldTypeClass::BreakAnim`, `BreakShield`'s fallback list.
    pub(crate) fn break_anim(&self) -> &[String] {
        &self.break_anim
    }

    /// Phobos `ShieldTypeClass::HitAnim`, `WeaponNullifyAnim`'s fallback.
    pub(crate) fn hit_anim(&self) -> &[String] {
        &self.hit_anim
    }

    /// Phobos `ShieldTypeClass::BreakWeapon`, the break shot's fallback.
    pub(crate) fn break_weapon(&self) -> Option<&str> {
        self.break_weapon.as_deref()
    }

    /// Phobos `ShieldTypeClass::ImmuneToCrit`.
    pub(crate) fn immune_to_crit(&self) -> bool {
        self.immune_to_crit
    }

    pub(crate) fn absorb_percent(&self) -> f64 {
        self.absorb_percent
    }

    pub(crate) fn pass_percent(&self) -> f64 {
        self.pass_percent
    }

    pub(crate) fn received_damage_minimum(&self) -> i32 {
        self.received_damage_minimum
    }

    pub(crate) fn received_damage_maximum(&self) -> i32 {
        self.received_damage_maximum
    }

    /// `Pips`/`Pips.Building`: the green/yellow/red PIPS.SHP frames, each
    /// `-1` when unset. The global `[AudioVisual] Pips.Shield*` list covers an
    /// entry whose own X is `-1`.
    pub fn pips(&self, building: bool) -> [i32; 3] {
        if building {
            self.pips_building
        } else {
            self.pips
        }
    }

    pub fn pips_background(&self) -> Option<&str> {
        self.pips_background.as_deref()
    }

    pub fn pips_building_empty(&self) -> Option<i32> {
        self.pips_building_empty
    }

    pub fn pips_hide_if_no_strength(&self) -> bool {
        self.pips_hide_if_no_strength
    }

    pub fn bracket_delta(&self) -> i32 {
        self.bracket_delta
    }

    pub fn tint_color(&self) -> Option<(u8, u8, u8)> {
        self.tint_color
    }

    pub fn tint_intensity(&self) -> f64 {
        self.tint_intensity
    }

    pub fn tint_visible_to_houses(&self) -> AffectedHouse {
        self.tint_visible_to_houses
    }
}

/// Float fields hash by bits so equal entries hash equal.
impl std::hash::Hash for ShieldType {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.strength.hash(state);
        self.initial_strength.hash(state);
        self.condition_yellow.map(f64::to_bits).hash(state);
        self.condition_red.map(f64::to_bits).hash(state);
        self.armor.hash(state);
        self.inherit_armor_from_techno.hash(state);
        self.inherit_armor_allowed.hash(state);
        self.inherit_armor_disallowed.hash(state);
        self.apply_armor_mult.hash(state);
        self.powered.hash(state);
        self.respawn.to_bits().hash(state);
        self.respawn_rate.hash(state);
        self.respawn_restart_in_combat.hash(state);
        self.respawn_restart_in_combat_delay.hash(state);
        self.respawn_anim.hash(state);
        self.respawn_weapon.hash(state);
        self.self_healing.to_bits().hash(state);
        self.self_healing_rate.hash(state);
        self.self_healing_restart_in_combat.hash(state);
        self.self_healing_restart_in_combat_delay.hash(state);
        self.self_healing_enabled_by.hash(state);
        self.absorb_over_damage.hash(state);
        self.immune_to_berserk.hash(state);
        self.immune_to_crit.hash(state);
        self.bracket_delta.hash(state);
        self.break_anim.hash(state);
        self.hit_anim.hash(state);
        self.hit_flash.hash(state);
        self.hit_flash_fixed_size.hash(state);
        self.hit_flash_red.hash(state);
        self.hit_flash_green.hash(state);
        self.hit_flash_blue.hash(state);
        self.hit_flash_black.hash(state);
        self.break_weapon.hash(state);
        self.absorb_percent.to_bits().hash(state);
        self.pass_percent.to_bits().hash(state);
        self.received_damage_minimum.hash(state);
        self.received_damage_maximum.hash(state);
        self.pips.hash(state);
        self.pips_building.hash(state);
        self.pips_background.hash(state);
        self.pips_building_empty.hash(state);
        self.pips_hide_if_no_strength.hash(state);
        self.tint_color.hash(state);
        self.tint_intensity.to_bits().hash(state);
        self.tint_visible_to_houses.hash(state);
        self.allow_transfer.hash(state);
        self.allow_transfer_convert.hash(state);
    }
}

/// Ordered Phobos `Enumerable<ShieldTypeClass>::Array`. Unlike the radiation
/// catalog it has no seeded default: `ShieldTypeClass` defines no
/// `AddDefaults`, and a techno without `ShieldType=` has no shield at all.
#[derive(Debug, Clone, PartialEq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub struct ShieldTypeCatalog {
    types: Vec<ShieldType>,
}

impl ShieldTypeCatalog {
    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        self.types.iter().map(|entry| entry.name.as_str())
    }

    pub fn entry(&self, name: &str) -> Option<&ShieldType> {
        self.types
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
    }

    /// Read one reached Rules pass: the pass's `[ShieldTypes]` list appends
    /// entries, then every retained entry re-reads its own section.
    pub(crate) fn apply_pass(&mut self, pass: &IniFile) {
        if let Some(list) = pass.section("ShieldTypes") {
            // Phobos Enumerable::LoadFromINIList reads each physical key
            // through ReadString with Phobos::readBuffer[2048] before lookup.
            // Source: Phobos b90ab4bc, src/Utilities/Enumerable.h:49-60,
            // src/Phobos.h:23-24; CCINIClass::ReadString @0x00528A10.
            for key in list.physical_keys() {
                let Some(name) = list.read_name(key, 2048) else {
                    continue;
                };
                if self.entry(name).is_none() {
                    self.types.push(ShieldType::named(&truncate_native_bytes(
                        name,
                        NAME_PAYLOAD_BYTES,
                    )));
                }
            }
        }
        for index in 0..self.types.len() {
            let name = self.types[index].name.clone();
            let Some(section) = pass.section(&name) else {
                continue;
            };
            let entry = &mut self.types[index];
            entry.strength = section.read_int("Strength", entry.strength);
            entry.initial_strength =
                section.read_phobos_nullable_int("InitialStrength", entry.initial_strength);
            entry.condition_yellow =
                section.read_phobos_nullable_double("ConditionYellow", entry.condition_yellow);
            entry.condition_red =
                section.read_phobos_nullable_double("ConditionRed", entry.condition_red);
            entry.idle_anim.apply(section, "IdleAnim");
            entry.idle_anim_damaged.apply(section, "IdleAnimDamaged");
            entry.idle_anim_offline_action = section.read_string_with(
                "IdleAnim.OfflineAction",
                0x40,
                entry.idle_anim_offline_action,
                |current, raw| AttachedAnimFlag::parse(raw).unwrap_or(current),
            );
            entry.idle_anim_temporal_action = section.read_string_with(
                "IdleAnim.TemporalAction",
                0x40,
                entry.idle_anim_temporal_action,
                |current, raw| AttachedAnimFlag::parse(raw).unwrap_or(current),
            );
            entry.armor = section.read_string("Armor", &entry.armor, 0x80);
            entry.inherit_armor_from_techno =
                section.read_bool("InheritArmorFromTechno", entry.inherit_armor_from_techno);
            if let Some(tokens) = section.read_list("InheritArmor.Allowed", 0x80) {
                entry.inherit_armor_allowed = tokens.into_iter().map(str::to_owned).collect();
            }
            if let Some(tokens) = section.read_list("InheritArmor.Disallowed", 0x80) {
                entry.inherit_armor_disallowed = tokens.into_iter().map(str::to_owned).collect();
            }
            entry.apply_armor_mult =
                section.read_phobos_nullable_bool("ApplyArmorMult", entry.apply_armor_mult);
            entry.powered = section.read_bool("Powered", entry.powered);
            entry.respawn = section.read_double("Respawn", entry.respawn);
            // ShieldTypeClass::LoadFromINI uses a fresh Nullable<double>
            // scratch per pass; unset/reset/invalid rates retain stored frames.
            if let Some(minutes) = section.read_phobos_nullable_double("Respawn.Rate", None) {
                entry.respawn_rate = (minutes * 900.0) as i32;
            }
            entry.respawn_restart_in_combat =
                section.read_bool("Respawn.RestartInCombat", entry.respawn_restart_in_combat);
            entry.respawn_restart_in_combat_delay = section.read_int(
                "Respawn.RestartInCombatDelay",
                entry.respawn_restart_in_combat_delay,
            );
            if let Some(tokens) = section.read_list("Respawn.Anim", 0x80) {
                entry.respawn_anim = tokens.into_iter().map(str::to_owned).collect();
            }
            if section.is_present("Respawn.Weapon") {
                entry.respawn_weapon = section
                    .read_name("Respawn.Weapon", 0x80)
                    .filter(|name| !is_none_name(name))
                    .map(str::to_owned);
            }
            entry.self_healing = section.read_double("SelfHealing", entry.self_healing);
            if let Some(minutes) = section.read_phobos_nullable_double("SelfHealing.Rate", None) {
                entry.self_healing_rate = (minutes * 900.0) as i32;
            }
            entry.self_healing_restart_in_combat = section.read_bool(
                "SelfHealing.RestartInCombat",
                entry.self_healing_restart_in_combat,
            );
            entry.self_healing_restart_in_combat_delay = section.read_int(
                "SelfHealing.RestartInCombatDelay",
                entry.self_healing_restart_in_combat_delay,
            );
            if let Some(tokens) = section.read_list("SelfHealing.EnabledBy", 0x80) {
                entry.self_healing_enabled_by = tokens.into_iter().map(str::to_owned).collect();
            }
            entry.absorb_over_damage =
                section.read_bool("AbsorbOverDamage", entry.absorb_over_damage);
            entry.immune_to_berserk = section.read_bool("ImmuneToBerserk", entry.immune_to_berserk);
            entry.immune_to_crit = section.read_bool("ImmuneToCrit", entry.immune_to_crit);
            entry.bracket_delta = section.read_int("BracketDelta", entry.bracket_delta);
            entry.break_anim = section
                .read_list("BreakAnim", 0x80)
                .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
                .unwrap_or_else(|| entry.break_anim.clone());
            entry.hit_anim = section
                .read_list("HitAnim", 0x80)
                .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
                .unwrap_or_else(|| entry.hit_anim.clone());
            entry.hit_flash = section.read_bool("HitFlash", entry.hit_flash);
            entry.hit_flash_fixed_size =
                section.read_phobos_nullable_int("HitFlash.FixedSize", entry.hit_flash_fixed_size);
            entry.hit_flash_red = section.read_bool("HitFlash.Red", entry.hit_flash_red);
            entry.hit_flash_green = section.read_bool("HitFlash.Green", entry.hit_flash_green);
            entry.hit_flash_blue = section.read_bool("HitFlash.Blue", entry.hit_flash_blue);
            entry.hit_flash_black = section.read_bool("HitFlash.Black", entry.hit_flash_black);
            if section.is_present("BreakWeapon") {
                entry.break_weapon = section
                    .read_name("BreakWeapon", 0x80)
                    .filter(|name| !is_none_name(name))
                    .map(str::to_owned);
            }
            entry.absorb_percent = section.read_double("AbsorbPercent", entry.absorb_percent);
            entry.pass_percent = section.read_double("PassPercent", entry.pass_percent);
            // `ShieldTypeClass.cpp:75-76`, read after PassPercent and before
            // the Pips block.
            entry.allow_transfer =
                section.read_phobos_nullable_bool("AllowTransfer", entry.allow_transfer);
            entry.allow_transfer_convert = section
                .read_phobos_nullable_bool("AllowTransfer.Convert", entry.allow_transfer_convert);
            entry.received_damage_minimum =
                section.read_int("ReceivedDamage.Minimum", entry.received_damage_minimum);
            entry.received_damage_maximum =
                section.read_int("ReceivedDamage.Maximum", entry.received_damage_maximum);
            entry.pips = read_pip_triplet(section, "Pips", entry.pips);
            entry.pips_building = read_pip_triplet(section, "Pips.Building", entry.pips_building);
            entry.pips_background = section.read_phobos_nullable_with(
                "Pips.Background",
                entry.pips_background.take(),
                |name| Some(name.to_owned()),
            );
            entry.pips_building_empty =
                section.read_phobos_nullable_int("Pips.Building.Empty", entry.pips_building_empty);
            entry.pips_hide_if_no_strength =
                section.read_bool("Pips.HideIfNoStrength", entry.pips_hide_if_no_strength);
            if section.is_present("Tint.Color") {
                let [red, green, blue] = section.read_color_rgb("Tint.Color", [0, 0, 0], None);
                entry.tint_color = Some((red, green, blue));
            }
            entry.tint_intensity = section.read_double("Tint.Intensity", entry.tint_intensity);
            if section.is_present("Tint.VisibleToHouses") {
                let raw = section.read_string("Tint.VisibleToHouses", "", 0x100);
                if let Some(houses) = AffectedHouse::parse(&raw) {
                    entry.tint_visible_to_houses = houses;
                }
            }
        }
    }
}

/// The warhead-side shield overrides (`WarheadTypeExt` shield fields,
/// `src/Ext/WarheadType/Body.cpp:246..287`, defaults `Body.h:371..410`).
/// Read on the merged warhead section; `None` floats/ints retain the shield
/// type's value at use, exactly as `Nullable.Get(typeValue)` resolves them.
/// The `ShieldTypeClass*` lists store raw tokens: the native readers parse
/// each comma token through `Parser<T>::Parse` (`Utilities/TemplateDef.h`
/// `detail::parse_values`), which drops names with no type, so the catalog
/// filter at the use site reproduces the parsed vector.
#[derive(Debug, Clone, PartialEq)]
pub struct ShieldWarheadRules {
    pub(crate) penetrate: bool,
    /// `Shield.AffectTypes`: raw tokens; an empty parsed list reaches every
    /// shield type.
    pub(crate) affect_types: Vec<String>,
    /// `Shield.Penetrate.Types` (`NullableVector`): `None` is unset, where
    /// `GetElements(Shield_AffectTypes)` falls back to the affect list.
    pub(crate) penetrate_types: Option<Vec<String>>,
    pub(crate) absorb_percent: Option<f64>,
    pub(crate) pass_percent: Option<f64>,
    pub(crate) received_minimum: Option<i32>,
    pub(crate) received_maximum: Option<i32>,
    pub(crate) received_min_multiplier: f64,
    pub(crate) received_max_multiplier: f64,
    /// `Shield.Break`: `ApplyShieldModifiers` breaks the eligible active shield.
    pub(crate) break_shield: bool,
    /// `Shield.Break.Types` (`NullableVector`): the break arm's type filter.
    pub(crate) break_types: Option<Vec<String>>,
    /// `Shield.BreakAnim`: the break animation list, `BreakShield`'s first
    /// argument; an empty parsed list falls back to the type's `BreakAnim`.
    pub(crate) break_anim: Vec<String>,
    /// `Shield.BreakWeapon` (`Nullable<WeaponTypeClass*>`, `Read<true>`):
    /// `None` falls back to the shield type's `BreakWeapon` at the break.
    pub(crate) break_weapon: Option<String>,
    /// `Shield.HitAnim`: the shield-absorbed hit animation list, an empty
    /// parsed list falls back to the type's `HitAnim`.
    pub(crate) hit_anim: Vec<String>,
    /// `Shield.SkipHitAnim`: suppresses the absorbed-hit animation.
    pub(crate) skip_hit_anim: bool,
    /// `Shield.HitFlash` (default true): the shield owner's flash gate. VERA
    /// ports the key; the `FlashbangWarheadAt` spotlight stays a render-side
    /// residual.
    pub(crate) hit_flash: bool,
    /// `Shield.Respawn.*`: the warhead's respawn recovery modifier.
    pub(crate) respawn_duration: i32,
    pub(crate) respawn_amount: Option<f64>,
    /// `Shield.Respawn.Rate` in minutes (`Valueable<double>` default -1.0);
    /// the frames value is `(int)(minutes * 900)` (`Body.cpp:263`).
    pub(crate) respawn_rate_in_minutes: f64,
    pub(crate) respawn_rate: i32,
    pub(crate) respawn_restart_in_combat: Option<bool>,
    pub(crate) respawn_restart_in_combat_delay: i32,
    pub(crate) respawn_restart_timer: bool,
    pub(crate) respawn_anim: Vec<String>,
    /// `Shield.Respawn.Weapon` (`Nullable<WeaponTypeClass*>`, plain `Find`).
    pub(crate) respawn_weapon: Option<String>,
    /// `Shield.Respawn.Types` (`NullableVector`): the respawn arm's type filter.
    pub(crate) respawn_types: Option<Vec<String>>,
    /// `Shield.SelfHealing.*`: the warhead's self-healing recovery modifier.
    pub(crate) self_healing_duration: i32,
    pub(crate) self_healing_amount: Option<f64>,
    pub(crate) self_healing_rate_in_minutes: f64,
    pub(crate) self_healing_rate: i32,
    pub(crate) self_healing_restart_in_combat: Option<bool>,
    pub(crate) self_healing_restart_in_combat_delay: i32,
    pub(crate) self_healing_restart_timer: bool,
    /// `Shield.SelfHealing.Types` (`NullableVector`).
    pub(crate) self_healing_types: Option<Vec<String>>,
    /// `Shield.AttachTypes`/`Shield.RemoveTypes` (`ValueableVector`).
    pub(crate) attach_types: Vec<String>,
    pub(crate) remove_types: Vec<String>,
    pub(crate) remove_all: bool,
    pub(crate) replace_only: bool,
    pub(crate) replace_non_respawning: bool,
    pub(crate) inherit_state_on_replace: bool,
    pub(crate) minimum_replace_delay: i32,
}

impl Default for ShieldWarheadRules {
    fn default() -> Self {
        Self {
            penetrate: false,
            affect_types: Vec::new(),
            penetrate_types: None,
            absorb_percent: None,
            pass_percent: None,
            received_minimum: None,
            received_maximum: None,
            received_min_multiplier: 1.0,
            received_max_multiplier: 1.0,
            break_shield: false,
            break_types: None,
            break_anim: Vec::new(),
            break_weapon: None,
            hit_anim: Vec::new(),
            skip_hit_anim: false,
            hit_flash: true,
            respawn_duration: 0,
            respawn_amount: None,
            respawn_rate_in_minutes: -1.0,
            respawn_rate: -1,
            respawn_restart_in_combat: None,
            respawn_restart_in_combat_delay: -1,
            respawn_restart_timer: false,
            respawn_anim: Vec::new(),
            respawn_weapon: None,
            respawn_types: None,
            self_healing_duration: 0,
            self_healing_amount: None,
            self_healing_rate_in_minutes: -1.0,
            self_healing_rate: -1,
            self_healing_restart_in_combat: None,
            self_healing_restart_in_combat_delay: -1,
            self_healing_restart_timer: false,
            self_healing_types: None,
            attach_types: Vec::new(),
            remove_types: Vec::new(),
            remove_all: false,
            replace_only: false,
            replace_non_respawning: false,
            inherit_state_on_replace: false,
            minimum_replace_delay: 0,
        }
    }
}

impl ShieldWarheadRules {
    /// `WarheadTypeClass::ReadINI`'s shield block, in native order
    /// (`src/Ext/WarheadType/Body.cpp:247..287`). Phobos reads every key
    /// through `INI_EX::ReadString` into `Phobos::readBuffer[2048]`
    /// (`src/Phobos.h:26..28`).
    pub(crate) fn from_section(section: &crate::rules::ini_parser::IniSection) -> Self {
        let mut rules = Self::default();
        rules.penetrate = section.read_bool("Shield.Penetrate", rules.penetrate);
        rules.break_shield = section.read_bool("Shield.Break", rules.break_shield);
        rules.break_anim = read_phobos_name_list(section, "Shield.BreakAnim");
        rules.hit_anim = read_phobos_name_list(section, "Shield.HitAnim");
        rules.skip_hit_anim = section.read_bool("Shield.SkipHitAnim", rules.skip_hit_anim);
        rules.hit_flash = section.read_bool("Shield.HitFlash", rules.hit_flash);
        // `Nullable<WeaponTypeClass*>::Read<true>`: `none`/`<none>` answer
        // null, and an unknown name constructs the weapon on this pass.
        rules.break_weapon = section
            .read_type_name("Shield.BreakWeapon", 2048)
            .map(str::to_owned);
        rules.absorb_percent =
            section.read_phobos_nullable_double("Shield.AbsorbPercent", rules.absorb_percent);
        rules.pass_percent =
            section.read_phobos_nullable_double("Shield.PassPercent", rules.pass_percent);
        rules.received_minimum = section
            .read_phobos_nullable_int("Shield.ReceivedDamage.Minimum", rules.received_minimum);
        rules.received_maximum = section
            .read_phobos_nullable_int("Shield.ReceivedDamage.Maximum", rules.received_maximum);
        rules.received_min_multiplier = section.read_double(
            "Shield.ReceivedDamage.MinMultiplier",
            rules.received_min_multiplier,
        );
        rules.received_max_multiplier = section.read_double(
            "Shield.ReceivedDamage.MaxMultiplier",
            rules.received_max_multiplier,
        );
        rules.respawn_duration =
            section.read_int("Shield.Respawn.Duration", rules.respawn_duration);
        rules.respawn_amount =
            section.read_phobos_nullable_double("Shield.Respawn.Amount", rules.respawn_amount);
        // `Body.cpp:262..263`: the minutes field is a `Valueable<double>`;
        // every reached read recomputes `(int)(minutes * 900)`.
        rules.respawn_rate_in_minutes =
            section.read_phobos_double("Shield.Respawn.Rate", rules.respawn_rate_in_minutes);
        rules.respawn_rate = (rules.respawn_rate_in_minutes * 900.0) as i32;
        rules.respawn_restart_in_combat = section.read_phobos_nullable_bool(
            "Shield.Respawn.RestartInCombat",
            rules.respawn_restart_in_combat,
        );
        rules.respawn_restart_in_combat_delay = section.read_int(
            "Shield.Respawn.RestartInCombatDelay",
            rules.respawn_restart_in_combat_delay,
        );
        rules.respawn_restart_timer =
            section.read_bool("Shield.Respawn.RestartTimer", rules.respawn_restart_timer);
        rules.respawn_anim = read_phobos_name_list(section, "Shield.Respawn.Anim");
        rules.respawn_weapon = section
            .read_type_name("Shield.Respawn.Weapon", 2048)
            .map(str::to_owned);
        rules.respawn_types = section.read_phobos_nullable_type_list("Shield.Respawn.Types");
        rules.self_healing_duration =
            section.read_int("Shield.SelfHealing.Duration", rules.self_healing_duration);
        rules.self_healing_amount = section
            .read_phobos_nullable_double("Shield.SelfHealing.Amount", rules.self_healing_amount);
        rules.self_healing_rate_in_minutes = section.read_phobos_double(
            "Shield.SelfHealing.Rate",
            rules.self_healing_rate_in_minutes,
        );
        rules.self_healing_rate = (rules.self_healing_rate_in_minutes * 900.0) as i32;
        rules.self_healing_restart_in_combat = section.read_phobos_nullable_bool(
            "Shield.SelfHealing.RestartInCombat",
            rules.self_healing_restart_in_combat,
        );
        rules.self_healing_restart_in_combat_delay = section.read_int(
            "Shield.SelfHealing.RestartInCombatDelay",
            rules.self_healing_restart_in_combat_delay,
        );
        rules.self_healing_restart_timer = section.read_bool(
            "Shield.SelfHealing.RestartTimer",
            rules.self_healing_restart_timer,
        );
        rules.attach_types = read_phobos_name_list(section, "Shield.AttachTypes");
        rules.remove_types = read_phobos_name_list(section, "Shield.RemoveTypes");
        rules.remove_all = section.read_bool("Shield.RemoveAll", rules.remove_all);
        rules.replace_only = section.read_bool("Shield.ReplaceOnly", rules.replace_only);
        rules.replace_non_respawning =
            section.read_bool("Shield.ReplaceNonRespawning", rules.replace_non_respawning);
        rules.inherit_state_on_replace = section.read_bool(
            "Shield.InheritStateOnReplace",
            rules.inherit_state_on_replace,
        );
        rules.minimum_replace_delay =
            section.read_int("Shield.MinimumReplaceDelay", rules.minimum_replace_delay);
        rules.affect_types = read_phobos_name_list(section, "Shield.AffectTypes");
        rules.penetrate_types = section.read_phobos_nullable_type_list("Shield.Penetrate.Types");
        rules.break_types = section.read_phobos_nullable_type_list("Shield.Break.Types");
        rules.self_healing_types =
            section.read_phobos_nullable_type_list("Shield.SelfHealing.Types");
        rules
    }

    /// `PossibleCellSpreadDetonate`'s shield members
    /// (`src/Ext/WarheadType/Body.cpp:495..516`): the per-target detonation
    /// pass reaches a warhead that carries any shield attach/remove/break or
    /// a respawn/self-healing duration.
    pub(crate) fn has_per_target_modifier(&self) -> bool {
        self.break_shield
            || self.respawn_duration > 0
            || self.self_healing_duration > 0
            || !self.attach_types.is_empty()
            || !self.remove_types.is_empty()
            || self.remove_all
    }
}

fn is_none_name(name: &str) -> bool {
    name.is_empty() || name.eq_ignore_ascii_case("none") || name.eq_ignore_ascii_case("<none>")
}

/// `ValueableVector<T*>::Read` (`Utilities/TemplateDef.h:1947..1956`): a
/// reached read replaces the vector, parsing each comma token through
/// `Parser<T>::Parse`; unknown names drop and an absent key keeps the
/// current list. Raw tokens; the catalog filter runs at the use site.
pub(crate) fn read_phobos_name_list(
    section: &crate::rules::ini_parser::IniSection,
    key: &str,
) -> Vec<String> {
    section
        .read_list(key, 2048)
        .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// `Vector3D<int>` read: the first three tokens replace the defaults; an
/// absent key or short list keeps them.
fn read_pip_triplet(
    section: &crate::rules::ini_parser::IniSection,
    key: &str,
    default: [i32; 3],
) -> [i32; 3] {
    let Some(values) = section.read_int_list(key) else {
        return default;
    };
    let mut out = default;
    for (slot, value) in out.iter_mut().zip(values.iter()) {
        *slot = *value;
    }
    out
}

/// `[CombatDamage] ShieldApplyArmorMult` (default false) and
/// `[AudioVisual] Shield.ConditionYellow`/`ConditionRed`, which fall back to
/// the ordinary `[AudioVisual] ConditionYellow`/`ConditionRed`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShieldGlobals {
    pub(crate) apply_armor_mult: bool,
    pub(crate) condition_yellow: f64,
    pub(crate) condition_red: f64,
    pub(crate) pips: [i32; 3],
    pub(crate) pips_building: [i32; 3],
    pub(crate) pips_background: Option<String>,
    pub(crate) pips_building_empty: Option<i32>,
}

impl Default for ShieldGlobals {
    fn default() -> Self {
        Self {
            apply_armor_mult: false,
            condition_yellow: 0.5,
            condition_red: 0.25,
            pips: [-1, -1, -1],
            pips_building: [-1, -1, -1],
            pips_background: None,
            pips_building_empty: None,
        }
    }
}

impl ShieldGlobals {
    pub(crate) fn from_ini(ini: &crate::rules::ini_parser::IniFile) -> Self {
        let defaults = Self::default();
        let combat = ini.section_or_empty("CombatDamage");
        let audio = ini.section_or_empty("AudioVisual");
        let condition_yellow = audio.read_double("ConditionYellow", defaults.condition_yellow);
        let condition_red = audio.read_double("ConditionRed", defaults.condition_red);
        Self {
            apply_armor_mult: combat.read_bool("ShieldApplyArmorMult", defaults.apply_armor_mult),
            condition_yellow: audio
                .read_phobos_nullable_double("Shield.ConditionYellow", None)
                .unwrap_or(condition_yellow),
            condition_red: audio
                .read_phobos_nullable_double("Shield.ConditionRed", None)
                .unwrap_or(condition_red),
            pips: read_pip_triplet(&audio, "Pips.Shield", defaults.pips),
            pips_building: read_pip_triplet(&audio, "Pips.Shield.Building", defaults.pips_building),
            pips_background: audio.read_phobos_nullable_with(
                "Pips.Shield.Background",
                defaults.pips_background,
                |name| Some(name.to_owned()),
            ),
            pips_building_empty: audio.read_phobos_nullable_int(
                "Pips.Shield.Building.Empty",
                defaults.pips_building_empty,
            ),
        }
    }

    pub fn apply_armor_mult(&self) -> bool {
        self.apply_armor_mult
    }

    pub fn condition_yellow(&self) -> f64 {
        self.condition_yellow
    }

    pub fn condition_red(&self) -> f64 {
        self.condition_red
    }

    /// `[AudioVisual] Pips.Shield`: green/yellow/red PIPS.SHP frames.
    pub fn pips(&self, building: bool) -> [i32; 3] {
        if building {
            self.pips_building
        } else {
            self.pips
        }
    }

    pub fn pips_background(&self) -> Option<&str> {
        self.pips_background.as_deref()
    }

    pub fn pips_building_empty(&self) -> Option<i32> {
        self.pips_building_empty
    }
}

#[cfg(all(test, tests_rules_o_z, feature = "phobos"))]
#[path = "phobos_shields_tests.rs"]
mod tests;
