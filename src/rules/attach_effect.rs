//! Phobos AttachEffect types, retained by the Rules registry.
//!
//! Native identity: Phobos `AttachEffectTypeClass`
//! (`src/New/Type/AttachEffectTypeClass.{h,cpp}`), an
//! `Enumerable<AttachEffectTypeClass>` whose main section is
//! `[AttachEffectTypes]`. `AttachEffectTypeClass::LoadFromINI` re-reads every
//! retained entry's own section on each reached Rules pass, keeping the
//! previous value for an absent key.
//!
//! This module starts the port with the catalog and the fields the first
//! slices consume: identity, duration, stacking, power gating and the target
//! filters. Discard conditions, animations, multipliers and weapons add their
//! own keys here as their slices land, so the reader stays one owner.

#[cfg(feature = "phobos")]
use crate::rules::ini_parser::IniFile;
#[cfg(feature = "phobos")]
use crate::rules::ini_value::truncate_native_bytes;
use crate::rules::phobos_shields::AffectedHouse;

/// `Enumerable` stores `Name[32]` with `_TRUNCATE`.
#[cfg(feature = "phobos")]
const NAME_PAYLOAD_BYTES: usize = 0x1F;

/// `CCINIClass::GetSection` compares the CRC of the uppercased name, so a
/// catalog entry resolves its own section whatever case the file used.
#[cfg(feature = "phobos")]
fn section_named<'a>(
    pass: &'a IniFile,
    name: &str,
) -> Option<&'a crate::rules::ini_parser::IniSection> {
    pass.section(name).or_else(|| {
        pass.section_names()
            .into_iter()
            .find(|candidate| candidate.eq_ignore_ascii_case(name))
            .and_then(|candidate| pass.section(candidate))
    })
}

/// Phobos `AffectsTarget` techno-type groups. An empty list would let every
/// category through; the reader maps `none` to no bits and `all` to every bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AttachEffectTargets {
    pub infantry: bool,
    pub units: bool,
    pub aircraft: bool,
    pub buildings: bool,
}

impl AttachEffectTargets {
    pub const ALL: Self = Self {
        infantry: true,
        units: true,
        aircraft: true,
        buildings: true,
    };
}

impl Default for AttachEffectTargets {
    fn default() -> Self {
        Self::ALL
    }
}

impl AttachEffectTargets {
    /// `EnumFunctions::IsTechnoEligible` with `considerAircraftSeparately`:
    /// the class flags decide, and `none` (no bits) admits nobody. This struct
    /// carries only the four content bits, so the native `All` early return and
    /// the `None` fallback are the same as this match.
    pub fn allows(self, category: crate::rules::object_type::ObjectCategory) -> bool {
        use crate::rules::object_type::ObjectCategory;
        match category {
            ObjectCategory::Infantry => self.infantry,
            ObjectCategory::Vehicle => self.units,
            ObjectCategory::Aircraft => self.aircraft,
            ObjectCategory::Building => self.buildings,
        }
    }

    /// Phobos `TechnoTypeFlags` enumeration names, in declaration order.
    /// Unknown tokens keep the previous value, as the native list read does.
    #[cfg(feature = "phobos")]
    fn from_tokens(current: Self, tokens: &[&str]) -> Self {
        let mut next = Self {
            infantry: false,
            units: false,
            aircraft: false,
            buildings: false,
        };
        for token in tokens {
            match token.to_ascii_lowercase().as_str() {
                "all" => next = Self::ALL,
                "none" => {}
                "infantry" => next.infantry = true,
                "units" => next.units = true,
                "aircraft" => next.aircraft = true,
                "buildings" => next.buildings = true,
                _ => return current,
            }
        }
        next
    }
}

/// Phobos `AttachedAnimFlag`: what the effect's animation does while the
/// owner is offline or the target is under a temporal freeze.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum AttachedAnimFlag {
    #[default]
    None,
    Hides,
    Temporal,
    Paused,
    PausedTemporal,
}

impl AttachedAnimFlag {
    #[cfg(feature = "phobos")]
    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "hides" => Some(Self::Hides),
            "temporal" => Some(Self::Temporal),
            "paused" => Some(Self::Paused),
            "pausedtemporal" => Some(Self::PausedTemporal),
            _ => None,
        }
    }
}

/// Phobos `DiscardCondition` flags, in declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct DiscardOn(u32);

impl DiscardOn {
    pub const NONE: Self = Self(0);
    pub const ENTRY: Self = Self(0x1);
    pub const MOVE: Self = Self(0x2);
    pub const STATIONARY: Self = Self(0x4);
    pub const DRAIN: Self = Self(0x8);
    pub const IN_RANGE: Self = Self(0x10);
    pub const OUT_OF_RANGE: Self = Self(0x20);
    pub const FIRING: Self = Self(0x40);
    pub const SELLING: Self = Self(0x80);
    pub const UNDEPLOYING: Self = Self(0x100);
    pub const HARVESTING: Self = Self(0x200);
    pub const INVOKER_DIE: Self = Self(0x400);
    pub const AMMO: Self = Self(0x800);
    pub const HEALTH: Self = Self(0x1000);
    pub const MISSION: Self = Self(0x2000);
    pub const LAND_TYPE: Self = Self(0x4000);
    pub const SEQUENCE: Self = Self(0x8000);
    pub const RECEIVED_DAMAGE: Self = Self(0x10000);
    pub const OWNER_CHANGE: Self = Self(0x20000);

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    pub fn is_none(self) -> bool {
        self.0 == 0
    }

    /// `read<DiscardCondition>`: comma-separated tokens, unknown tokens
    /// ignored, `none` contributes no bit.
    #[cfg(feature = "phobos")]
    fn parse(raw: &str) -> Self {
        let mut parsed = Self::NONE;
        for token in raw.split(|c: char| matches!(c, ',' | ' ' | '\t' | '\r')) {
            parsed.0 |= match token.trim().to_ascii_lowercase().as_str() {
                "entry" => Self::ENTRY,
                "move" => Self::MOVE,
                "stationary" => Self::STATIONARY,
                "drain" => Self::DRAIN,
                "inrange" => Self::IN_RANGE,
                "outofrange" => Self::OUT_OF_RANGE,
                "firing" => Self::FIRING,
                "selling" => Self::SELLING,
                "undeploying" => Self::UNDEPLOYING,
                "harvesting" => Self::HARVESTING,
                "invokerdie" => Self::INVOKER_DIE,
                "ammo" => Self::AMMO,
                "health" => Self::HEALTH,
                "mission" => Self::MISSION,
                "landtype" => Self::LAND_TYPE,
                "sequence" => Self::SEQUENCE,
                "receiveddamage" => Self::RECEIVED_DAMAGE,
                "ownerchange" => Self::OWNER_CHANGE,
                _ => Self::NONE,
            }
            .0;
        }
        parsed
    }
}

/// Phobos `ExpireWeaponCondition` flags, in declaration order. `All` is the
/// literal `0xFF` mask, so it covers every condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExpireWeaponCondition(u8);

impl ExpireWeaponCondition {
    pub const NONE: Self = Self(0);
    pub const EXPIRE: Self = Self(0x1);
    pub const REMOVE: Self = Self(0x2);
    pub const DEATH: Self = Self(0x4);
    pub const DISCARD: Self = Self(0x8);
    pub const ALL: Self = Self(0xFF);

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// `read<ExpireWeaponCondition>`: comma-separated tokens. A token outside
    /// the enumeration makes the whole read fail, so the caller keeps its
    /// current value (unlike `DiscardOn`, whose parser ignores it).
    #[cfg(feature = "phobos")]
    fn parse(raw: &str) -> Option<Self> {
        let mut parsed = Self::NONE;
        for token in raw.split(|c: char| matches!(c, ',' | ' ' | '\t' | '\r')) {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            parsed.0 |= match token.to_ascii_lowercase().as_str() {
                "none" => Self::NONE,
                "expire" => Self::EXPIRE,
                "remove" => Self::REMOVE,
                "death" => Self::DEATH,
                "discard" => Self::DISCARD,
                "all" => Self::ALL,
                _ => return None,
            }
            .0;
        }
        Some(parsed)
    }
}

impl Default for ExpireWeaponCondition {
    /// The constructor's `ExpireWeapon.TriggerOn=expire`.
    fn default() -> Self {
        Self::EXPIRE
    }
}

/// One `Animation.DrawOffsetN` rule. The first absent or `0,0` index ends the
/// list, so the array is always contiguous from index 0.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AnimationDrawOffset {
    pub offset: [i32; 2],
    pub required_types: Vec<String>,
}

/// One `[AttachEffectTypes]` entry. Defaults are the constructor's
/// (`AttachEffectTypeClass.h`), never retail values.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AttachEffectType {
    pub name: String,
    /// `Duration`: game frames, negative for an indefinite effect.
    pub duration: i32,
    pub duration_apply_firepower_mult: bool,
    pub duration_apply_armor_mult_on_target: bool,
    pub cumulative: bool,
    /// `Cumulative.MaxCount`; negative means no cap.
    pub cumulative_max_count: i32,
    pub powered: bool,
    pub penetrates_iron_curtain: bool,
    /// `PenetratesForceShield`; `None` inherits `penetrates_iron_curtain`.
    pub penetrates_force_shield: Option<bool>,
    /// `AffectTypes`: whitelist, highest priority when non-empty.
    pub affect_types: Vec<String>,
    /// `IgnoreTypes`: blacklist, checked after `affect_types`.
    pub ignore_types: Vec<String>,
    pub affects_target: AttachEffectTargets,
    /// `Groups`: comma-separated group ids.
    pub groups: Vec<String>,
    /// `AllowTransfer`; `None` defaults to whether the effect came from
    /// outside (not self-owned).
    pub allow_transfer: Option<bool>,
    /// `AllowTransfer.Convert`; `None` defaults to `allow_transfer`.
    pub allow_transfer_convert: Option<bool>,
    /// `Animation`: the looping animation's `[Animations]` name.
    pub animation: Option<String>,
    /// `CumulativeAnimations`: one animation per stacked layer; the last
    /// covers every deeper layer.
    pub cumulative_animations: Vec<String>,
    pub cumulative_animations_restart_on_change: bool,
    /// `CumulativeAnimations.CountIncrement`: above 1 the layer count divides
    /// by it instead of indexing from one; LoadFromINI clamps it to at least 1.
    pub cumulative_animations_count_increment: i32,
    pub animation_reset_on_reapply: bool,
    pub animation_offline_action: AttachedAnimFlag,
    pub animation_temporal_action: AttachedAnimFlag,
    pub animation_use_invoker_as_owner: bool,
    pub animation_hide_if_attached_with: Vec<String>,
    pub animation_draw_offsets: Vec<AnimationDrawOffset>,
    /// `Tint.Color`; `None` leaves the target untinted.
    pub tint_color: Option<[u8; 3]>,
    pub tint_intensity: f64,
    pub tint_visible_to_houses: AffectedHouse,
    /// `Tint.Cumulative`: a cumulative effect's tints of the same type add
    /// up instead of the type tinting once.
    pub tint_cumulative: bool,
    /// `FirepowerMultiplier`.
    pub firepower_multiplier: f64,
    /// `ArmorMultiplier` and its conditional form.
    pub armor_multiplier: f64,
    pub armor_multiplier_allow_warheads: Vec<String>,
    pub armor_multiplier_disallow_warheads: Vec<String>,
    pub armor_multiplier_chance: f64,
    /// `ArmorMultiplier.Delay`: a real hit that applied this multiplier blocks
    /// later applications for this many frames.
    pub armor_multiplier_delay: i32,
    pub armor_multiplier_affects_house: AffectedHouse,
    /// `ArmorMultiplier.HitAnim`: one is picked when the multiplier applies to
    /// a received hit.
    pub armor_multiplier_hit_anim: Vec<String>,
    /// `SpeedMultiplier`.
    pub speed_multiplier: f64,
    /// `ROFMultiplier`; `.ApplyOnCurrentTimer` scales the timer already
    /// running when the effect initializes.
    pub rof_multiplier: f64,
    pub rof_multiplier_apply_on_current_timer: bool,
    pub cloakable: bool,
    pub force_decloak: bool,
    /// `WeaponRange.Multiplier` applies first, `.ExtraRange` (in cells) adds
    /// after every effect's multiplier, and the two weapon lists gate both.
    pub weapon_range_multiplier: f64,
    pub weapon_range_extra_range: f64,
    pub weapon_range_allow_weapons: Vec<String>,
    pub weapon_range_disallow_weapons: Vec<String>,
    /// `Crit.Multiplier` / `Crit.ExtraChance`, gated by warhead.
    pub crit_multiplier: f64,
    pub crit_extra_chance: f64,
    pub crit_allow_warheads: Vec<String>,
    pub crit_disallow_warheads: Vec<String>,
    pub disable_weapons: bool,
    pub unkillable: bool,
    /// `LaserTrail.Type`: natively a `ValueableIdx<LaserTrailTypeClass>`
    /// resolved against `[LaserTrailTypes]` (`AttachEffectTypeClass.cpp:184`).
    /// VERA has no LaserTrail owner or trail catalog yet, so the raw key text
    /// is retained and no trail is spawned.
    pub laser_trail_type: Option<String>,
    /// `ExpireWeapon`: the weapon this effect fires when it ends. `.TriggerOn`
    /// is the flag mask of the ends that fire it, `.CumulativeOnlyOnce` fires
    /// a cumulative type once at its last ending, and `.UseInvokerAsOwner`
    /// gives the shot the invoker's identity.
    pub expire_weapon: Option<String>,
    pub expire_weapon_trigger_on: ExpireWeaponCondition,
    pub expire_weapon_cumulative_only_once: bool,
    pub expire_weapon_use_invoker_as_owner: bool,
    /// `RevengeWeapon`: fired at whoever dealt the killing damage.
    pub revenge_weapon: Option<String>,
    pub revenge_weapon_affects_house: AffectedHouse,
    pub revenge_weapon_use_invoker_as_owner: bool,
    /// `ReflectDamage`: every received hit can be dealt back to its source
    /// with `.Warhead` (default `[CombatDamage] C4Warhead`), at
    /// `.Multiplier` or the fixed `.Override`, under `.Chance` and
    /// `.AffectsHouse`. `.Warhead.Detonate` detonates at the source instead of
    /// calling its receiver.
    pub reflect_damage: bool,
    pub reflect_damage_warhead: Option<String>,
    pub reflect_damage_warhead_detonate: bool,
    pub reflect_damage_multiplier: f64,
    pub reflect_damage_affects_house: AffectedHouse,
    pub reflect_damage_chance: f64,
    /// `ReflectDamage.Delay`: a successful reflection blocks later ones for
    /// this many frames.
    pub reflect_damage_delay: i32,
    pub reflect_damage_override: Option<i32>,
    pub reflect_damage_use_invoker_as_owner: bool,
    /// `DiscardOn` and the keys its conditions read.
    pub discard_on: DiscardOn,
    pub discard_ammo_minimum: i32,
    pub discard_ammo_maximum: i32,
    pub discard_health_below: f64,
    pub discard_health_above: f64,
    pub discard_firing_count: i32,
    pub discard_received_damage_count: i32,
    pub discard_received_damage_affects_house: AffectedHouse,
    pub discard_missions: Vec<String>,
    /// `DiscardOn.AIMissions`; `None` inherits `discard_missions`.
    pub discard_ai_missions: Option<Vec<String>>,
    pub discard_land_types: Vec<String>,
    pub discard_sequences: Vec<String>,
    /// `DiscardOn.Sequences.Immediate`; `None` inherits `[General]`.
    pub discard_sequences_immediate: Option<bool>,
    /// `DiscardOn.RangeOverride`, in cells.
    pub discard_range_override: Option<f64>,
    /// `DiscardOn.MoveBasedOnDestination`; `None` inherits `[General]`.
    pub discard_move_based_on_destination: Option<bool>,
    /// `DiscardOn.ConsiderHarvestingAsStationary`; `None` inherits `[General]`.
    pub discard_consider_harvesting_as_stationary: Option<bool>,
    pub discard_owner_change_human_to_computer: bool,
    pub discard_owner_change_computer_to_human: bool,
    pub discard_owner_change_ignore_revert_on_exit: bool,
}

impl Default for AttachEffectType {
    /// The constructor's defaults with no catalog name.
    fn default() -> Self {
        Self::named("")
    }
}

impl AttachEffectType {
    fn named(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            duration: 0,
            duration_apply_firepower_mult: false,
            duration_apply_armor_mult_on_target: false,
            cumulative: false,
            cumulative_max_count: -1,
            powered: false,
            penetrates_iron_curtain: false,
            penetrates_force_shield: None,
            affect_types: Vec::new(),
            ignore_types: Vec::new(),
            affects_target: AttachEffectTargets::ALL,
            groups: Vec::new(),
            allow_transfer: None,
            allow_transfer_convert: None,
            animation: None,
            cumulative_animations: Vec::new(),
            cumulative_animations_restart_on_change: true,
            cumulative_animations_count_increment: 1,
            animation_reset_on_reapply: false,
            animation_offline_action: AttachedAnimFlag::Hides,
            animation_temporal_action: AttachedAnimFlag::None,
            animation_use_invoker_as_owner: false,
            animation_hide_if_attached_with: Vec::new(),
            animation_draw_offsets: Vec::new(),
            tint_color: None,
            tint_intensity: 0.0,
            tint_visible_to_houses: AffectedHouse::ALL,
            tint_cumulative: true,
            firepower_multiplier: 1.0,
            armor_multiplier: 1.0,
            armor_multiplier_allow_warheads: Vec::new(),
            armor_multiplier_disallow_warheads: Vec::new(),
            armor_multiplier_chance: 1.0,
            armor_multiplier_delay: 0,
            armor_multiplier_affects_house: AffectedHouse::ALL,
            armor_multiplier_hit_anim: Vec::new(),
            speed_multiplier: 1.0,
            rof_multiplier: 1.0,
            rof_multiplier_apply_on_current_timer: true,
            cloakable: false,
            force_decloak: false,
            weapon_range_multiplier: 1.0,
            weapon_range_extra_range: 0.0,
            weapon_range_allow_weapons: Vec::new(),
            weapon_range_disallow_weapons: Vec::new(),
            crit_multiplier: 1.0,
            crit_extra_chance: 0.0,
            crit_allow_warheads: Vec::new(),
            crit_disallow_warheads: Vec::new(),
            disable_weapons: false,
            unkillable: false,
            laser_trail_type: None,
            expire_weapon: None,
            expire_weapon_trigger_on: ExpireWeaponCondition::default(),
            expire_weapon_cumulative_only_once: false,
            expire_weapon_use_invoker_as_owner: false,
            revenge_weapon: None,
            revenge_weapon_affects_house: AffectedHouse::ALL,
            revenge_weapon_use_invoker_as_owner: false,
            reflect_damage: false,
            reflect_damage_warhead: None,
            reflect_damage_warhead_detonate: false,
            reflect_damage_multiplier: 1.0,
            reflect_damage_affects_house: AffectedHouse::ALL,
            reflect_damage_chance: 1.0,
            reflect_damage_delay: 0,
            reflect_damage_override: None,
            reflect_damage_use_invoker_as_owner: false,
            discard_on: DiscardOn::NONE,
            discard_ammo_minimum: -1,
            discard_ammo_maximum: -1,
            discard_health_below: -1.0,
            discard_health_above: -1.0,
            discard_firing_count: 1,
            discard_received_damage_count: 1,
            discard_received_damage_affects_house: AffectedHouse::ALL,
            discard_missions: Vec::new(),
            discard_ai_missions: None,
            discard_land_types: Vec::new(),
            discard_sequences: Vec::new(),
            discard_sequences_immediate: None,
            discard_range_override: None,
            discard_move_based_on_destination: None,
            discard_consider_harvesting_as_stationary: None,
            discard_owner_change_human_to_computer: true,
            discard_owner_change_computer_to_human: true,
            discard_owner_change_ignore_revert_on_exit: false,
        }
    }

    /// `GetCumulativeAnimation(count)` (`AttachEffectTypeClass.h:241`): above
    /// a `CountIncrement` of 1 the layer count divides by it, otherwise it
    /// indexes from one; every layer past the list keeps the last one.
    pub fn cumulative_animation(&self, count: i32) -> Option<&str> {
        if count < 1 || self.cumulative_animations.is_empty() {
            return None;
        }
        let size = self.cumulative_animations.len() as i32;
        let increment = self.cumulative_animations_count_increment.max(1);
        let index = if increment > 1 {
            (count / increment).clamp(0, size - 1)
        } else {
            (count - 1).min(size - 1)
        };
        self.cumulative_animations
            .get(index as usize)
            .map(String::as_str)
    }

    /// `HasAnim()`: a cumulative type shows through its cumulative list, every
    /// other type through its single `Animation`.
    pub fn has_animation(&self) -> bool {
        if self.cumulative {
            !self.cumulative_animations.is_empty() || self.animation.is_some()
        } else {
            self.animation.is_some()
        }
    }

    /// `HasTint()`: a colour or a nonzero intensity.
    pub fn has_tint(&self) -> bool {
        self.tint_color.is_some() || self.tint_intensity != 0.0
    }

    /// `RestrictedArmorMultiplier`: the armor factor is only rolled at hit
    /// time when a hit anim is listed or any gate (chance, delay, warhead
    /// lists, house) can reject it.
    pub fn restricted_armor_multiplier(&self) -> bool {
        !self.armor_multiplier_hit_anim.is_empty()
            || (self.armor_multiplier != 1.0
                && (!self.armor_multiplier_allow_warheads.is_empty()
                    || !self.armor_multiplier_disallow_warheads.is_empty()
                    || self.armor_multiplier_chance < 1.0
                    || self.armor_multiplier_delay > 0
                    || self.armor_multiplier_affects_house != AffectedHouse::ALL))
    }

    /// `GetRangeWithModifiers` skips a type whose multiplier is 1.0 and whose
    /// extra range is 0.
    pub fn has_range_modifier(&self) -> bool {
        self.weapon_range_multiplier != 1.0 || self.weapon_range_extra_range != 0.0
    }

    /// `HasCritModifiers`.
    pub fn has_crit_modifiers(&self) -> bool {
        self.crit_multiplier != 1.0 || self.crit_extra_chance != 0.0
    }

    /// `AllowTransfer.Get(!IsSelfOwned())` (`AttachEffectClass.cpp:1231`,
    /// `Body.Update.cpp:1084`): an effect attached from outside defaults to
    /// carrying across a transfer or conversion, one a techno attaches to
    /// itself does not. `AllowTransfer.Convert` splits the conversion case
    /// out; only the deploy/undeploy transfer consumes the base key today,
    /// because no Ares type conversion is ported.
    pub fn allows_transfer(&self, self_owned: bool, converting: bool) -> bool {
        let base = self.allow_transfer.unwrap_or(!self_owned);
        if converting {
            self.allow_transfer_convert.unwrap_or(base)
        } else {
            base
        }
    }

    /// `PenetratesForceShield` falls back to `PenetratesIronCurtain` when the
    /// key is absent or blank.
    pub fn penetrates_force_shield(&self) -> bool {
        self.penetrates_force_shield
            .unwrap_or(self.penetrates_iron_curtain)
    }
}

/// The `[AttachEffectTypes]` catalog, retained across Rules passes.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AttachEffectTypeCatalog {
    types: Vec<AttachEffectType>,
}

impl AttachEffectTypeCatalog {
    pub fn types(&self) -> &[AttachEffectType] {
        &self.types
    }

    #[cfg(feature = "phobos")]
    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        self.types.iter().map(|entry| entry.name.as_str())
    }

    pub fn entry(&self, name: &str) -> Option<&AttachEffectType> {
        self.types
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
    }

    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.types
            .iter()
            .position(|entry| entry.name.eq_ignore_ascii_case(name))
    }

    /// Read one reached Rules pass: the pass's `[AttachEffectTypes]` list
    /// appends entries, then every retained entry re-reads its own section.
    #[cfg(feature = "phobos")]
    pub(crate) fn apply_pass(&mut self, pass: &IniFile) {
        if let Some(list) = pass.section("AttachEffectTypes") {
            for key in list.physical_keys() {
                let Some(name) = list.read_name(key, 2048) else {
                    continue;
                };
                if self.entry(name).is_none() {
                    self.types
                        .push(AttachEffectType::named(&truncate_native_bytes(
                            name,
                            NAME_PAYLOAD_BYTES,
                        )));
                }
            }
        }
        for index in 0..self.types.len() {
            let name = self.types[index].name.clone();
            let Some(section) = section_named(pass, &name) else {
                continue;
            };
            let entry = &mut self.types[index];
            entry.duration = section.read_int("Duration", entry.duration);
            entry.duration_apply_firepower_mult = section.read_bool(
                "Duration.ApplyFirepowerMult",
                entry.duration_apply_firepower_mult,
            );
            entry.duration_apply_armor_mult_on_target = section.read_bool(
                "Duration.ApplyArmorMultOnTarget",
                entry.duration_apply_armor_mult_on_target,
            );
            entry.cumulative = section.read_bool("Cumulative", entry.cumulative);
            entry.cumulative_max_count =
                section.read_int("Cumulative.MaxCount", entry.cumulative_max_count);
            entry.powered = section.read_bool("Powered", entry.powered);
            entry.penetrates_iron_curtain =
                section.read_bool("PenetratesIronCurtain", entry.penetrates_iron_curtain);
            entry.penetrates_force_shield = section
                .read_phobos_nullable_bool("PenetratesForceShield", entry.penetrates_force_shield);
            if let Some(tokens) = section.read_list("AffectTypes", 0x800) {
                entry.affect_types = tokens.iter().map(|name| (*name).to_owned()).collect();
            }
            if let Some(tokens) = section.read_list("IgnoreTypes", 0x800) {
                entry.ignore_types = tokens.iter().map(|name| (*name).to_owned()).collect();
            }
            // `AffectTargets` is the deprecated spelling and only warns.
            if let Some(tokens) = section
                .read_list("AffectsTarget", 0x800)
                .or_else(|| section.read_list("AffectTargets", 0x800))
            {
                entry.affects_target =
                    AttachEffectTargets::from_tokens(entry.affects_target, &tokens);
            }
            entry.allow_transfer =
                section.read_phobos_nullable_bool("AllowTransfer", entry.allow_transfer);
            entry.allow_transfer_convert = section
                .read_phobos_nullable_bool("AllowTransfer.Convert", entry.allow_transfer_convert);
            entry.animation = section
                .read_name("Animation", 0x100)
                .filter(|name| !name.is_empty())
                .map(str::to_owned);
            if let Some(tokens) = section.read_list("CumulativeAnimations", 0x800) {
                entry.cumulative_animations =
                    tokens.iter().map(|name| (*name).to_owned()).collect();
            }
            entry.cumulative_animations_restart_on_change = section.read_bool(
                "CumulativeAnimations.RestartOnChange",
                entry.cumulative_animations_restart_on_change,
            );
            // LoadFromINI's post-read correction: a value below 1 becomes 1.
            entry.cumulative_animations_count_increment = section
                .read_int(
                    "CumulativeAnimations.CountIncrement",
                    entry.cumulative_animations_count_increment,
                )
                .max(1);
            entry.animation_reset_on_reapply =
                section.read_bool("Animation.ResetOnReapply", entry.animation_reset_on_reapply);
            entry.animation_offline_action = section.read_string_with(
                "Animation.OfflineAction",
                0x40,
                entry.animation_offline_action,
                |current, raw| AttachedAnimFlag::parse(raw).unwrap_or(current),
            );
            entry.animation_temporal_action = section.read_string_with(
                "Animation.TemporalAction",
                0x40,
                entry.animation_temporal_action,
                |current, raw| AttachedAnimFlag::parse(raw).unwrap_or(current),
            );
            entry.animation_use_invoker_as_owner = section.read_bool(
                "Animation.UseInvokerAsOwner",
                entry.animation_use_invoker_as_owner,
            );
            if let Some(tokens) = section.read_list("Animation.HideIfAttachedWith", 0x800) {
                entry.animation_hide_if_attached_with =
                    tokens.iter().map(|name| (*name).to_owned()).collect();
            }
            entry.tint_color =
                section.read_phobos_nullable_with("Tint.Color", entry.tint_color, |raw| {
                    Some(crate::rules::ini_parser::IniSection::read_color_rgb_value(
                        raw,
                        [0, 0, 0],
                    ))
                });
            entry.tint_intensity =
                section.read_phobos_double("Tint.Intensity", entry.tint_intensity);
            entry.tint_visible_to_houses = section.read_string_with(
                "Tint.VisibleToHouses",
                0x100,
                entry.tint_visible_to_houses,
                |current, raw| AffectedHouse::parse(raw).unwrap_or(current),
            );
            entry.tint_cumulative = section.read_bool("Tint.Cumulative", entry.tint_cumulative);
            entry.firepower_multiplier =
                section.read_phobos_double("FirepowerMultiplier", entry.firepower_multiplier);
            entry.armor_multiplier =
                section.read_phobos_double("ArmorMultiplier", entry.armor_multiplier);
            if let Some(tokens) = section.read_list("ArmorMultiplier.AllowWarheads", 0x800) {
                entry.armor_multiplier_allow_warheads =
                    tokens.iter().map(|name| (*name).to_owned()).collect();
            }
            if let Some(tokens) = section.read_list("ArmorMultiplier.DisallowWarheads", 0x800) {
                entry.armor_multiplier_disallow_warheads =
                    tokens.iter().map(|name| (*name).to_owned()).collect();
            }
            entry.armor_multiplier_chance =
                section.read_phobos_double("ArmorMultiplier.Chance", entry.armor_multiplier_chance);
            entry.armor_multiplier_delay =
                section.read_int("ArmorMultiplier.Delay", entry.armor_multiplier_delay);
            entry.armor_multiplier_affects_house = section.read_string_with(
                "ArmorMultiplier.AffectsHouse",
                0x100,
                entry.armor_multiplier_affects_house,
                |current, raw| AffectedHouse::parse(raw).unwrap_or(current),
            );
            if let Some(tokens) = section.read_list("ArmorMultiplier.HitAnim", 0x800) {
                entry.armor_multiplier_hit_anim =
                    tokens.iter().map(|name| (*name).to_owned()).collect();
            }
            entry.speed_multiplier =
                section.read_phobos_double("SpeedMultiplier", entry.speed_multiplier);
            entry.rof_multiplier =
                section.read_phobos_double("ROFMultiplier", entry.rof_multiplier);
            entry.rof_multiplier_apply_on_current_timer = section.read_bool(
                "ROFMultiplier.ApplyOnCurrentTimer",
                entry.rof_multiplier_apply_on_current_timer,
            );
            entry.cloakable = section.read_bool("Cloakable", entry.cloakable);
            entry.force_decloak = section.read_bool("ForceDecloak", entry.force_decloak);
            entry.weapon_range_multiplier =
                section.read_phobos_double("WeaponRange.Multiplier", entry.weapon_range_multiplier);
            entry.weapon_range_extra_range = section
                .read_phobos_double("WeaponRange.ExtraRange", entry.weapon_range_extra_range);
            if let Some(tokens) = section.read_list("WeaponRange.AllowWeapons", 0x800) {
                entry.weapon_range_allow_weapons =
                    tokens.iter().map(|name| (*name).to_owned()).collect();
            }
            if let Some(tokens) = section.read_list("WeaponRange.DisallowWeapons", 0x800) {
                entry.weapon_range_disallow_weapons =
                    tokens.iter().map(|name| (*name).to_owned()).collect();
            }
            entry.crit_multiplier =
                section.read_phobos_double("Crit.Multiplier", entry.crit_multiplier);
            entry.crit_extra_chance =
                section.read_phobos_double("Crit.ExtraChance", entry.crit_extra_chance);
            if let Some(tokens) = section.read_list("Crit.AllowWarheads", 0x800) {
                entry.crit_allow_warheads = tokens.iter().map(|name| (*name).to_owned()).collect();
            }
            if let Some(tokens) = section.read_list("Crit.DisallowWarheads", 0x800) {
                entry.crit_disallow_warheads =
                    tokens.iter().map(|name| (*name).to_owned()).collect();
            }
            entry.disable_weapons = section.read_bool("DisableWeapons", entry.disable_weapons);
            entry.unkillable = section.read_bool("Unkillable", entry.unkillable);
            entry.laser_trail_type = section
                .read_name("LaserTrail.Type", 2048)
                .filter(|name| !name.is_empty())
                .map(str::to_owned);
            // `ExpireWeapon.Read<true>` and friends: the type factories answer
            // null for an empty or `none` name, so an absent key keeps the
            // retained pointer.
            if section.is_present("ExpireWeapon") {
                entry.expire_weapon = section
                    .read_type_name("ExpireWeapon", 0x100)
                    .map(str::to_owned);
            }
            entry.expire_weapon_trigger_on = section.read_string_with(
                "ExpireWeapon.TriggerOn",
                0x100,
                entry.expire_weapon_trigger_on,
                |current, raw| ExpireWeaponCondition::parse(raw).unwrap_or(current),
            );
            entry.expire_weapon_cumulative_only_once = section.read_bool(
                "ExpireWeapon.CumulativeOnlyOnce",
                entry.expire_weapon_cumulative_only_once,
            );
            entry.expire_weapon_use_invoker_as_owner = section.read_bool(
                "ExpireWeapon.UseInvokerAsOwner",
                entry.expire_weapon_use_invoker_as_owner,
            );
            if section.is_present("RevengeWeapon") {
                entry.revenge_weapon = section
                    .read_type_name("RevengeWeapon", 0x100)
                    .map(str::to_owned);
            }
            // `.AffectsHouses` is the deprecated spelling; the underscore-free
            // key wins when both are present.
            entry.revenge_weapon_affects_house = section.read_string_with(
                "RevengeWeapon.AffectsHouses",
                0x100,
                entry.revenge_weapon_affects_house,
                |current, raw| AffectedHouse::parse(raw).unwrap_or(current),
            );
            entry.revenge_weapon_affects_house = section.read_string_with(
                "RevengeWeapon.AffectsHouse",
                0x100,
                entry.revenge_weapon_affects_house,
                |current, raw| AffectedHouse::parse(raw).unwrap_or(current),
            );
            entry.revenge_weapon_use_invoker_as_owner = section.read_bool(
                "RevengeWeapon.UseInvokerAsOwner",
                entry.revenge_weapon_use_invoker_as_owner,
            );
            entry.reflect_damage = section.read_bool("ReflectDamage", entry.reflect_damage);
            if section.is_present("ReflectDamage.Warhead") {
                entry.reflect_damage_warhead = section
                    .read_type_name("ReflectDamage.Warhead", 0x100)
                    .map(str::to_owned);
            }
            entry.reflect_damage_warhead_detonate = section.read_bool(
                "ReflectDamage.Warhead.Detonate",
                entry.reflect_damage_warhead_detonate,
            );
            entry.reflect_damage_multiplier = section
                .read_phobos_double("ReflectDamage.Multiplier", entry.reflect_damage_multiplier);
            entry.reflect_damage_affects_house = section.read_string_with(
                "ReflectDamage.AffectsHouses",
                0x100,
                entry.reflect_damage_affects_house,
                |current, raw| AffectedHouse::parse(raw).unwrap_or(current),
            );
            entry.reflect_damage_affects_house = section.read_string_with(
                "ReflectDamage.AffectsHouse",
                0x100,
                entry.reflect_damage_affects_house,
                |current, raw| AffectedHouse::parse(raw).unwrap_or(current),
            );
            entry.reflect_damage_chance =
                section.read_phobos_double("ReflectDamage.Chance", entry.reflect_damage_chance);
            entry.reflect_damage_delay =
                section.read_int("ReflectDamage.Delay", entry.reflect_damage_delay);
            entry.reflect_damage_override = section
                .read_phobos_nullable_int("ReflectDamage.Override", entry.reflect_damage_override);
            entry.reflect_damage_use_invoker_as_owner = section.read_bool(
                "ReflectDamage.UseInvokerAsOwner",
                entry.reflect_damage_use_invoker_as_owner,
            );
            entry.discard_on =
                section.read_string_with("DiscardOn", 0x400, entry.discard_on, |_, raw| {
                    DiscardOn::parse(raw)
                });
            entry.discard_ammo_minimum =
                section.read_int("DiscardOn.Ammo.MinimumAmount", entry.discard_ammo_minimum);
            entry.discard_ammo_maximum =
                section.read_int("DiscardOn.Ammo.MaximumAmount", entry.discard_ammo_maximum);
            entry.discard_health_below = section
                .read_phobos_double("DiscardOn.Health.BelowPercent", entry.discard_health_below);
            entry.discard_health_above = section
                .read_phobos_double("DiscardOn.Health.AbovePercent", entry.discard_health_above);
            entry.discard_firing_count =
                section.read_int("DiscardOn.Firing.Count", entry.discard_firing_count);
            entry.discard_received_damage_count = section.read_int(
                "DiscardOn.ReceivedDamage.Count",
                entry.discard_received_damage_count,
            );
            entry.discard_received_damage_affects_house = section.read_string_with(
                "DiscardOn.ReceivedDamage.AffectsHouse",
                0x100,
                entry.discard_received_damage_affects_house,
                |current, raw| AffectedHouse::parse(raw).unwrap_or(current),
            );
            if let Some(tokens) = section.read_list("DiscardOn.Missions", 0x800) {
                entry.discard_missions = tokens.iter().map(|name| (*name).to_owned()).collect();
            }
            entry.discard_ai_missions = section
                .read_list("DiscardOn.AIMissions", 0x800)
                .map(|tokens| tokens.iter().map(|name| (*name).to_owned()).collect());
            if let Some(tokens) = section.read_list("DiscardOn.LandTypes", 0x800) {
                entry.discard_land_types = tokens.iter().map(|name| (*name).to_owned()).collect();
            }
            if let Some(tokens) = section.read_list("DiscardOn.Sequences", 0x800) {
                entry.discard_sequences = tokens.iter().map(|name| (*name).to_owned()).collect();
            }
            entry.discard_sequences_immediate = section.read_phobos_nullable_bool(
                "DiscardOn.Sequences.Immediate",
                entry.discard_sequences_immediate,
            );
            entry.discard_range_override = section.read_phobos_nullable_double(
                "DiscardOn.RangeOverride",
                entry.discard_range_override,
            );
            entry.discard_move_based_on_destination = section.read_phobos_nullable_bool(
                "DiscardOn.MoveBasedOnDestination",
                entry.discard_move_based_on_destination,
            );
            entry.discard_consider_harvesting_as_stationary = section.read_phobos_nullable_bool(
                "DiscardOn.ConsiderHarvestingAsStationary",
                entry.discard_consider_harvesting_as_stationary,
            );
            entry.discard_owner_change_human_to_computer = section.read_bool(
                "DiscardOn.OwnerChange.HumanToComputer",
                entry.discard_owner_change_human_to_computer,
            );
            entry.discard_owner_change_computer_to_human = section.read_bool(
                "DiscardOn.OwnerChange.ComputerToHuman",
                entry.discard_owner_change_computer_to_human,
            );
            entry.discard_owner_change_ignore_revert_on_exit = section.read_bool(
                "DiscardOn.OwnerChange.IgnoreRevertOnExit",
                entry.discard_owner_change_ignore_revert_on_exit,
            );
            // The native loop appends into the retained vector, which would
            // duplicate every rule on a second Rules pass; rebuild instead.
            entry.animation_draw_offsets.clear();
            for index in 0.. {
                let key = format!("Animation.DrawOffset{index}");
                let offset = section.read_phobos_int_pair(&key, [0, 0]);
                if offset == [0, 0] {
                    break;
                }
                let required_types = section
                    .read_list(&format!("{key}.RequiredTypes"), 0x800)
                    .map(|tokens| tokens.iter().map(|name| (*name).to_owned()).collect())
                    .unwrap_or_default();
                entry.animation_draw_offsets.push(AnimationDrawOffset {
                    offset,
                    required_types,
                });
            }
            if let Some(tokens) = section.read_list("Groups", 0x800) {
                entry.groups = tokens
                    .iter()
                    .map(|token| token.trim_matches(' ').to_owned())
                    .collect();
            }
        }
    }
}

/// The `[General]` switches this slice consumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct AttachEffectGlobals {
    pub discard_sequences_immediate: bool,
    pub discard_move_based_on_destination: bool,
    pub discard_consider_harvesting_as_stationary: bool,
    pub open_topped_use_transport_range_modifiers: bool,
    pub open_topped_check_transport_disable_weapons: bool,
    pub replace_longer_duration: bool,
    pub attach_on_owner_change: bool,
}

impl AttachEffectGlobals {
    /// Phobos `[General]` defaults. `DiscardOn.Sequences.Immediate` and
    /// `DiscardOn.ConsiderHarvestingAsStationary` default to `true`; the rest
    /// to `false`.
    pub fn from_ini(ini: &crate::rules::ini_parser::IniFile) -> Self {
        let Some(section) = ini.section("General") else {
            return Self {
                discard_sequences_immediate: true,
                discard_consider_harvesting_as_stationary: true,
                ..Self::default()
            };
        };
        let defaults = Self {
            discard_sequences_immediate: true,
            discard_consider_harvesting_as_stationary: true,
            ..Self::default()
        };
        Self {
            discard_sequences_immediate: section.read_bool(
                "DiscardOn.Sequences.Immediate",
                defaults.discard_sequences_immediate,
            ),
            discard_move_based_on_destination: section.read_bool(
                "DiscardOn.MoveBasedOnDestination",
                defaults.discard_move_based_on_destination,
            ),
            discard_consider_harvesting_as_stationary: section.read_bool(
                "DiscardOn.ConsiderHarvestingAsStationary",
                defaults.discard_consider_harvesting_as_stationary,
            ),
            open_topped_use_transport_range_modifiers: section.read_bool(
                "OpenTopped.UseTransportRangeModifiers",
                defaults.open_topped_use_transport_range_modifiers,
            ),
            open_topped_check_transport_disable_weapons: section.read_bool(
                "OpenTopped.CheckTransportDisableWeapons",
                defaults.open_topped_check_transport_disable_weapons,
            ),
            replace_longer_duration: section.read_bool(
                "AttachEffect.ReplaceLongerDuration",
                defaults.replace_longer_duration,
            ),
            attach_on_owner_change: section.read_bool(
                "AttachEffects.AttachOnOwnerChange",
                defaults.attach_on_owner_change,
            ),
        }
    }
}

#[cfg(all(test, feature = "phobos"))]
mod tests {
    use super::*;

    fn parse(text: &str) -> AttachEffectTypeCatalog {
        let ini = IniFile::from_str(text);
        let mut catalog = AttachEffectTypeCatalog::default();
        catalog.apply_pass(&ini);
        catalog
    }

    /// Animation and tint keys take the constructor's defaults, and the
    /// `Animation.DrawOffsetN` list stops at the first `0,0`.
    #[test]
    fn animation_and_tint_keys_read_with_documented_defaults() {
        let catalog = parse(
            "[AttachEffectTypes]
0=A
[A]
",
        );
        let entry = catalog.entry("A").expect("entry");
        assert_eq!(entry.animation, None);
        assert!(entry.cumulative_animations_restart_on_change);
        assert_eq!(entry.animation_offline_action, AttachedAnimFlag::Hides);
        assert_eq!(entry.animation_temporal_action, AttachedAnimFlag::None);
        assert_eq!(entry.tint_color, None);
        assert_eq!(entry.tint_intensity, 0.0);
        assert_eq!(entry.tint_visible_to_houses, AffectedHouse::ALL);
        assert!(entry.animation_draw_offsets.is_empty());
        assert_eq!(entry.firepower_multiplier, 1.0);
        assert_eq!(entry.armor_multiplier, 1.0);
        assert_eq!(entry.armor_multiplier_chance, 1.0);
        assert_eq!(entry.armor_multiplier_affects_house, AffectedHouse::ALL);
        assert!(entry.armor_multiplier_hit_anim.is_empty());
        assert_eq!(entry.speed_multiplier, 1.0);
        assert_eq!(entry.rof_multiplier, 1.0);
        assert!(entry.rof_multiplier_apply_on_current_timer);
        assert!(!entry.cloakable && !entry.force_decloak);
        assert_eq!(entry.weapon_range_multiplier, 1.0);
        assert_eq!(entry.weapon_range_extra_range, 0.0);
        assert_eq!(entry.crit_multiplier, 1.0);
        assert_eq!(entry.crit_extra_chance, 0.0);
        assert!(!entry.disable_weapons && !entry.unkillable);
        assert!(!entry.restricted_armor_multiplier());
        assert!(!entry.has_range_modifier());
        assert!(!entry.has_crit_modifiers());
        assert!(entry.discard_on.is_none());
        assert_eq!(entry.discard_ammo_minimum, -1);
        assert_eq!(entry.discard_ammo_maximum, -1);
        assert_eq!(entry.discard_health_below, -1.0);
        assert_eq!(entry.discard_health_above, -1.0);
        assert_eq!(entry.discard_firing_count, 1);
        assert_eq!(entry.discard_received_damage_count, 1);
        assert_eq!(
            entry.discard_received_damage_affects_house,
            AffectedHouse::ALL
        );
        assert!(entry.discard_ai_missions.is_none());
        assert!(entry.discard_sequences_immediate.is_none());
        assert!(entry.discard_range_override.is_none());
        assert!(entry.discard_move_based_on_destination.is_none());
        assert!(entry.discard_consider_harvesting_as_stationary.is_none());
        assert!(entry.discard_owner_change_human_to_computer);
        assert!(entry.discard_owner_change_computer_to_human);
        assert!(!entry.discard_owner_change_ignore_revert_on_exit);

        let catalog = parse(
            "[AttachEffectTypes]
0=B
[B]
Animation=LOOP
             CumulativeAnimations=ONE,TWO
CumulativeAnimations.RestartOnChange=no
             Animation.ResetOnReapply=yes
             Animation.OfflineAction=Paused
Animation.TemporalAction=PausedTemporal
             Animation.UseInvokerAsOwner=yes
Animation.HideIfAttachedWith=HIDE
             Tint.Color=10,20,30
Tint.Intensity=1.5
Tint.VisibleToHouses=owner
             Animation.DrawOffset0=7,-3
Animation.DrawOffset0.RequiredTypes=REQ
             Animation.DrawOffset1=2,0
Animation.DrawOffset2=0,0
",
        );
        let entry = catalog.entry("B").expect("entry");
        assert_eq!(entry.animation.as_deref(), Some("LOOP"));
        assert_eq!(entry.cumulative_animations, vec!["ONE", "TWO"]);
        assert!(!entry.cumulative_animations_restart_on_change);
        assert!(entry.animation_reset_on_reapply);
        assert_eq!(entry.animation_offline_action, AttachedAnimFlag::Paused);
        assert_eq!(
            entry.animation_temporal_action,
            AttachedAnimFlag::PausedTemporal
        );
        assert!(entry.animation_use_invoker_as_owner);
        assert_eq!(entry.animation_hide_if_attached_with, vec!["HIDE"]);
        assert_eq!(entry.tint_color, Some([10, 20, 30]));
        assert_eq!(entry.tint_intensity, 1.5);
        assert_eq!(entry.tint_visible_to_houses, AffectedHouse::OWNER);
        assert_eq!(entry.animation_draw_offsets.len(), 2, "0,0 ends the list");
        assert_eq!(entry.animation_draw_offsets[0].offset, [7, -3]);
        assert_eq!(entry.animation_draw_offsets[0].required_types, vec!["REQ"]);
        assert_eq!(entry.animation_draw_offsets[1].offset, [2, 0]);
    }

    /// The DiscardOn flag tokens are case-insensitive, unknown tokens are
    /// ignored, and every condition key reads its list or scalar.
    #[test]
    fn discard_keys_read_tokens_lists_and_defaults() {
        let catalog = parse(
            "[AttachEffectTypes]
0=D
[D]
DiscardOn=move, Ammo,health,none,bogus
DiscardOn.Ammo.MinimumAmount=2
DiscardOn.Ammo.MaximumAmount=6
DiscardOn.Health.BelowPercent=0.5
DiscardOn.Health.AbovePercent=0.2
DiscardOn.Firing.Count=3
DiscardOn.ReceivedDamage.Count=2
DiscardOn.ReceivedDamage.AffectsHouse=enemies
DiscardOn.Missions=Guard,Move
DiscardOn.AIMissions=Hunt
DiscardOn.LandTypes=Clear,Road
DiscardOn.Sequences=Walk,Attack
DiscardOn.Sequences.Immediate=no
DiscardOn.RangeOverride=1.5
DiscardOn.MoveBasedOnDestination=yes
DiscardOn.ConsiderHarvestingAsStationary=no
DiscardOn.OwnerChange.HumanToComputer=no
DiscardOn.OwnerChange.ComputerToHuman=no
DiscardOn.OwnerChange.IgnoreRevertOnExit=yes
",
        );
        let entry = catalog.entry("D").expect("entry");
        assert!(entry.discard_on.contains(DiscardOn::MOVE));
        assert!(entry.discard_on.contains(DiscardOn::AMMO));
        assert!(entry.discard_on.contains(DiscardOn::HEALTH));
        assert!(!entry.discard_on.contains(DiscardOn::MISSION));
        assert_eq!(entry.discard_ammo_minimum, 2);
        assert_eq!(entry.discard_ammo_maximum, 6);
        assert_eq!(entry.discard_health_below, 0.5);
        assert_eq!(
            entry.discard_health_above,
            f64::from(0.2_f32),
            "ReadDouble is f32"
        );
        assert_eq!(entry.discard_firing_count, 3);
        assert_eq!(entry.discard_received_damage_count, 2);
        assert_eq!(
            entry.discard_received_damage_affects_house,
            AffectedHouse::ENEMIES
        );
        assert_eq!(entry.discard_missions, vec!["Guard", "Move"]);
        assert_eq!(entry.discard_ai_missions, Some(vec!["Hunt".to_owned()]));
        assert_eq!(entry.discard_land_types, vec!["Clear", "Road"]);
        assert_eq!(entry.discard_sequences, vec!["Walk", "Attack"]);
        assert_eq!(entry.discard_sequences_immediate, Some(false));
        assert_eq!(entry.discard_range_override, Some(1.5));
        assert_eq!(entry.discard_move_based_on_destination, Some(true));
        assert_eq!(entry.discard_consider_harvesting_as_stationary, Some(false));
        assert!(!entry.discard_owner_change_human_to_computer);
        assert!(!entry.discard_owner_change_computer_to_human);
        assert!(entry.discard_owner_change_ignore_revert_on_exit);
    }

    /// The stat-multiplier keys read with their documented defaults, their
    /// name lists keep order, and the derived restriction flags follow the
    /// constructor's post-read pass.
    #[test]
    fn multiplier_keys_read_with_documented_defaults() {
        let catalog = parse(
            "[AttachEffectTypes]
0=M
[M]
FirepowerMultiplier=1.5
ArmorMultiplier=0.5
ArmorMultiplier.AllowWarheads=WH1,WH2
ArmorMultiplier.DisallowWarheads=WH3
ArmorMultiplier.Chance=0.25
ArmorMultiplier.AffectsHouse=enemies
ArmorMultiplier.HitAnim=HIT1
SpeedMultiplier=2.0
ROFMultiplier=0.75
ROFMultiplier.ApplyOnCurrentTimer=no
Cloakable=yes
ForceDecloak=yes
WeaponRange.Multiplier=1.25
WeaponRange.ExtraRange=2
WeaponRange.AllowWeapons=W1
WeaponRange.DisallowWeapons=W2
Crit.Multiplier=1.5
Crit.ExtraChance=0.1
Crit.AllowWarheads=WH1
Crit.DisallowWarheads=WH2
DisableWeapons=yes
Unkillable=yes
",
        );
        let entry = catalog.entry("M").expect("entry");
        assert_eq!(entry.firepower_multiplier, 1.5);
        assert_eq!(entry.armor_multiplier, 0.5);
        assert_eq!(entry.armor_multiplier_allow_warheads, vec!["WH1", "WH2"]);
        assert_eq!(entry.armor_multiplier_disallow_warheads, vec!["WH3"]);
        assert_eq!(entry.armor_multiplier_chance, 0.25);
        assert_eq!(entry.armor_multiplier_affects_house, AffectedHouse::ENEMIES);
        assert_eq!(entry.armor_multiplier_hit_anim, vec!["HIT1"]);
        assert!(entry.restricted_armor_multiplier());
        assert_eq!(entry.speed_multiplier, 2.0);
        assert_eq!(entry.rof_multiplier, 0.75);
        assert!(!entry.rof_multiplier_apply_on_current_timer);
        assert!(entry.cloakable && entry.force_decloak);
        assert_eq!(entry.weapon_range_multiplier, 1.25);
        assert_eq!(entry.weapon_range_extra_range, 2.0);
        assert_eq!(entry.weapon_range_allow_weapons, vec!["W1"]);
        assert_eq!(entry.weapon_range_disallow_weapons, vec!["W2"]);
        assert!(entry.has_range_modifier());
        assert_eq!(entry.crit_multiplier, 1.5);
        assert_eq!(
            entry.crit_extra_chance,
            f64::from(0.1_f32),
            "ReadDouble is f32"
        );
        assert_eq!(entry.crit_allow_warheads, vec!["WH1"]);
        assert_eq!(entry.crit_disallow_warheads, vec!["WH2"]);
        assert!(entry.has_crit_modifiers());
        assert!(entry.disable_weapons && entry.unkillable);

        // A bare multiplier with no gate stays unrestricted.
        let catalog = parse("[AttachEffectTypes]\n0=N\n[N]\nArmorMultiplier=0.5\n");
        assert!(!catalog.entry("N").unwrap().restricted_armor_multiplier());
        let catalog = parse("[AttachEffectTypes]\n0=O\n[O]\nArmorMultiplier.Chance=0.5\n");
        assert!(
            !catalog.entry("O").unwrap().restricted_armor_multiplier(),
            "the constructor's post-read pass needs a non-1.0 multiplier too"
        );
        let catalog = parse("[AttachEffectTypes]\n0=P\n[P]\nArmorMultiplier.HitAnim=HIT\n");
        assert!(
            catalog.entry("P").unwrap().restricted_armor_multiplier(),
            "a hit animation restricts on its own"
        );
    }

    /// The layer count picks a cumulative animation, the last entry covers
    /// deeper layers, and the animation/tint presence checks follow the
    /// constructor's shape.
    #[test]
    fn cumulative_animations_and_presence_follow_the_type() {
        let mut effect = AttachEffectType::default();
        assert_eq!(
            effect.cumulative_animation(1),
            None,
            "no list, no animation"
        );
        effect.cumulative = true;
        effect.cumulative_animations = vec!["A".to_owned(), "B".to_owned()];
        assert_eq!(effect.cumulative_animation(1), Some("A"));
        assert_eq!(effect.cumulative_animation(2), Some("B"));
        assert_eq!(
            effect.cumulative_animation(5),
            Some("B"),
            "the last covers the rest"
        );
        // `CountIncrement > 1` divides the layer count before indexing.
        effect.cumulative_animations = vec!["A".to_owned(), "B".to_owned(), "C".to_owned()];
        effect.cumulative_animations_count_increment = 3;
        assert_eq!(effect.cumulative_animation(1), Some("A"));
        assert_eq!(effect.cumulative_animation(2), Some("A"));
        assert_eq!(effect.cumulative_animation(3), Some("B"));
        assert_eq!(effect.cumulative_animation(5), Some("B"));
        assert_eq!(effect.cumulative_animation(6), Some("C"));
        assert_eq!(
            effect.cumulative_animation(30),
            Some("C"),
            "the last covers deeper layers"
        );
        assert!(effect.has_animation(), "a cumulative list counts as one");
        assert!(!effect.has_tint());
        effect.animation = Some("ONE".to_owned());
        assert!(effect.has_animation());
        effect.tint_intensity = 0.25;
        assert!(effect.has_tint(), "a nonzero intensity tints");
    }

    /// `CumulativeAnimations.CountIncrement` is clamped at 1, and the armor
    /// delay and tint-cumulative key keep their constructor defaults.
    #[test]
    fn count_increment_armor_delay_and_tint_cumulative_read() {
        let catalog = parse(
            "[AttachEffectTypes]\n0=T\n[T]\nCumulativeAnimations.CountIncrement=0\n\
             ArmorMultiplier.Delay=5\nTint.Cumulative=no\n",
        );
        let entry = catalog.entry("T").expect("entry");
        assert_eq!(
            entry.cumulative_animations_count_increment, 1,
            "below one is corrected to one"
        );
        assert_eq!(entry.armor_multiplier_delay, 5);
        assert!(!entry.tint_cumulative);

        let defaults = AttachEffectType::default();
        assert_eq!(defaults.cumulative_animations_count_increment, 1);
        assert_eq!(defaults.armor_multiplier_delay, 0);
        assert!(defaults.tint_cumulative);
    }

    /// `AllowTransfer` defaults to `!IsSelfOwned()`, and `.Convert` splits
    /// the conversion case out.
    #[test]
    fn allow_transfer_defaults_follow_ownership_and_convert() {
        let mut effect = AttachEffectType::default();
        assert!(!effect.allows_transfer(true, false), "self-owned stays");
        assert!(effect.allows_transfer(false, false), "attached carries");
        assert!(effect.allows_transfer(false, true));
        effect.allow_transfer = Some(false);
        assert!(!effect.allows_transfer(true, false));
        assert!(
            !effect.allows_transfer(true, true),
            "Convert inherits the base"
        );
        effect.allow_transfer_convert = Some(true);
        assert!(!effect.allows_transfer(true, false));
        assert!(effect.allows_transfer(true, true));

        let parsed = parse(
            "[AttachEffectTypes]
0=T
[T]
AllowTransfer=no
AllowTransfer.Convert=yes
",
        );
        let entry = parsed.entry("T").expect("entry");
        assert_eq!(entry.allow_transfer, Some(false));
        assert_eq!(entry.allow_transfer_convert, Some(true));
    }

    /// The catalog appends listed entries only, keeps constructor defaults for
    /// absent keys, and resolves `PenetratesForceShield` from the iron-curtain
    /// flag when the key is absent.
    #[test]
    fn catalog_reads_list_entries_and_keeps_constructor_defaults() {
        let catalog = parse(
            "[AttachEffectTypes]\n0=FIRST\n1=Second\n2=first\n[FIRST]\nDuration=48\n\
             Cumulative=yes\nCumulative.MaxCount=3\nPenetratesIronCurtain=yes\nGroups=tag,other\n\
             [SECOND]\nAffectsTarget=none\n",
        );
        assert_eq!(catalog.types().len(), 2, "duplicate name does not append");
        let first = catalog.entry("first").expect("case-insensitive lookup");
        assert_eq!(first.duration, 48);
        assert!(first.cumulative);
        assert_eq!(first.cumulative_max_count, 3);
        assert!(!first.duration_apply_firepower_mult, "constructor default");
        assert!(first.penetrates_iron_curtain);
        assert!(first.penetrates_force_shield(), "inherits the iron curtain");
        assert_eq!(first.groups, vec!["tag", "other"]);
        assert_eq!(first.affects_target, AttachEffectTargets::ALL);
        let second = catalog.entry("Second").expect("listed entry");
        assert_eq!(
            second.affects_target,
            AttachEffectTargets {
                infantry: false,
                units: false,
                aircraft: false,
                buildings: false,
            }
        );
        assert_eq!(catalog.index_of("SECOND"), Some(1));
    }

    /// `LaserTrail.Type` keeps the ART catalog key text (`ValueableIdx` in
    /// Phobos, resolved by name at use); an absent or blank key stays `None`.
    #[test]
    fn laser_trail_type_keeps_the_key_text() {
        let catalog = parse("[AttachEffectTypes]\n0=T\n[T]\nLaserTrail.Type=MyTrail\n");
        assert_eq!(
            catalog.entry("T").unwrap().laser_trail_type.as_deref(),
            Some("MyTrail")
        );
        let catalog = parse("[AttachEffectTypes]\n0=U\n[U]\nLaserTrail.Type=\n");
        assert_eq!(catalog.entry("U").unwrap().laser_trail_type, None);
    }

    /// `AffectsTarget` splits the four content classes and `none` admits
    /// nobody.
    #[test]
    fn target_class_flags_gate_each_category() {
        use crate::rules::object_type::ObjectCategory;
        let all = AttachEffectTargets::ALL;
        assert!(all.allows(ObjectCategory::Infantry));
        assert!(all.allows(ObjectCategory::Vehicle));
        assert!(all.allows(ObjectCategory::Aircraft));
        assert!(all.allows(ObjectCategory::Building));
        let none = AttachEffectTargets {
            infantry: false,
            units: false,
            aircraft: false,
            buildings: false,
        };
        assert!(!none.allows(ObjectCategory::Vehicle));
        let air_only = AttachEffectTargets {
            infantry: false,
            units: false,
            aircraft: true,
            buildings: false,
        };
        assert!(air_only.allows(ObjectCategory::Aircraft));
        assert!(!air_only.allows(ObjectCategory::Building));
    }

    /// A later pass re-reads the same entry and keeps a value whose key it no
    /// longer carries, while writing the keys it does carry.
    #[test]
    fn catalog_retains_absent_keys_across_passes() {
        let mut catalog = AttachEffectTypeCatalog::default();
        catalog.apply_pass(&IniFile::from_str(
            "[AttachEffectTypes]\n0=KEEP\n[KEEP]\nDuration=10\nPowered=yes\n",
        ));
        catalog.apply_pass(&IniFile::from_str("[KEEP]\nDuration=20\n"));
        let entry = catalog.entry("KEEP").expect("retained entry");
        assert_eq!(entry.duration, 20, "present key overwrites");
        assert!(entry.powered, "absent key keeps the previous value");
    }

    /// The catalog, the globals and the country/techno attach lists all
    /// survive the production Rules parse and are reachable from `RuleSet`.
    #[test]
    fn ruleset_retains_the_catalog_and_the_attach_lists() {
        use crate::rules::ruleset::RuleSet;

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]
AttachEffect.ReplaceLongerDuration=yes
             [AttachEffectTypes]
0=FIRST
[FIRST]
Duration=40
             [Countries]
0=Americans
[Americans]
AttachEffect.AttachTypes=NATIONAE
             [VehicleTypes]
0=MTNK
[MTNK]
Name=Apocalypse
             AttachEffect.AttachTypes=FIRST,SECOND
AttachEffect.Delays=5,7
",
        ))
        .expect("rules parse");

        assert_eq!(
            rules
                .attach_effect_types()
                .entry("FIRST")
                .map(|entry| entry.duration),
            Some(40)
        );
        assert!(rules.attach_effect_globals().replace_longer_duration);
        let techno = rules.object("MTNK").expect("techno");
        assert_eq!(
            techno.attach_effect_attach_types(),
            ["FIRST".to_owned(), "SECOND".to_owned()]
        );
        assert_eq!(techno.attach_effect_delays(), [5, 7]);
        assert_eq!(
            rules.country_attach_effect_types("Americans"),
            ["NATIONAE".to_owned()]
        );
    }

    /// The ExpireWeapon, RevengeWeapon and ReflectDamage keys: the constructor
    /// defaults, the condition flag parser, and the deprecated `AffectsHouses`
    /// spelling the underscore-free key overrides.
    #[test]
    fn weapon_and_reflect_keys_read_with_constructor_defaults() {
        let catalog = parse(
            "[AttachEffectTypes]
0=PLAIN
[PLAIN]
",
        );
        let entry = catalog.entry("PLAIN").expect("entry");
        assert_eq!(entry.expire_weapon, None);
        assert_eq!(
            entry.expire_weapon_trigger_on,
            ExpireWeaponCondition::EXPIRE,
            "the constructor's TriggerOn=expire"
        );
        assert!(!entry.expire_weapon_cumulative_only_once);
        assert!(!entry.expire_weapon_use_invoker_as_owner);
        assert_eq!(entry.revenge_weapon, None);
        assert_eq!(entry.revenge_weapon_affects_house, AffectedHouse::ALL);
        assert!(!entry.revenge_weapon_use_invoker_as_owner);
        assert!(!entry.reflect_damage);
        assert_eq!(entry.reflect_damage_warhead, None);
        assert!(!entry.reflect_damage_warhead_detonate);
        assert_eq!(entry.reflect_damage_multiplier, 1.0);
        assert_eq!(entry.reflect_damage_affects_house, AffectedHouse::ALL);
        assert_eq!(entry.reflect_damage_chance, 1.0);
        assert_eq!(entry.reflect_damage_override, None);
        assert!(!entry.reflect_damage_use_invoker_as_owner);

        let catalog = parse(
            "[AttachEffectTypes]
0=FULL
[FULL]
ExpireWeapon=DEATHBLAST
ExpireWeapon.TriggerOn=remove, death ,discard
ExpireWeapon.CumulativeOnlyOnce=yes
ExpireWeapon.UseInvokerAsOwner=yes
RevengeWeapon=PAYBACK
RevengeWeapon.AffectsHouses=enemies
RevengeWeapon.AffectsHouse=allies
RevengeWeapon.UseInvokerAsOwner=yes
ReflectDamage=yes
ReflectDamage.Warhead=SPIKE
ReflectDamage.Warhead.Detonate=yes
ReflectDamage.Multiplier=0.5
ReflectDamage.AffectsHouses=owner
ReflectDamage.AffectsHouse=enemies
ReflectDamage.Chance=0.25
ReflectDamage.Override=12
ReflectDamage.UseInvokerAsOwner=yes
",
        );
        let entry = catalog.entry("FULL").expect("entry");
        assert_eq!(entry.expire_weapon.as_deref(), Some("DEATHBLAST"));
        assert!(
            entry
                .expire_weapon_trigger_on
                .contains(ExpireWeaponCondition::REMOVE)
        );
        assert!(
            entry
                .expire_weapon_trigger_on
                .contains(ExpireWeaponCondition::DEATH)
        );
        assert!(
            entry
                .expire_weapon_trigger_on
                .contains(ExpireWeaponCondition::DISCARD)
        );
        assert!(
            !entry
                .expire_weapon_trigger_on
                .contains(ExpireWeaponCondition::EXPIRE)
        );
        assert!(entry.expire_weapon_cumulative_only_once);
        assert!(entry.expire_weapon_use_invoker_as_owner);
        assert_eq!(entry.revenge_weapon.as_deref(), Some("PAYBACK"));
        assert_eq!(
            entry.revenge_weapon_affects_house,
            AffectedHouse::ALLIES,
            "the underscore-free key wins"
        );
        assert!(entry.revenge_weapon_use_invoker_as_owner);
        assert!(entry.reflect_damage);
        assert_eq!(entry.reflect_damage_warhead.as_deref(), Some("SPIKE"));
        assert!(entry.reflect_damage_warhead_detonate);
        assert_eq!(entry.reflect_damage_multiplier, 0.5);
        assert_eq!(entry.reflect_damage_affects_house, AffectedHouse::ENEMIES);
        assert_eq!(entry.reflect_damage_chance, 0.25);
        assert_eq!(entry.reflect_damage_override, Some(12));
        assert!(entry.reflect_damage_use_invoker_as_owner);

        // `all` covers every condition, a valid later pass replaces the mask,
        // and an unknown token keeps it (the native reader fails the read).
        let mut catalog = AttachEffectTypeCatalog::default();
        catalog.apply_pass(&IniFile::from_str(
            "[AttachEffectTypes]\n0=T\n[T]\nExpireWeapon.TriggerOn=all\n",
        ));
        let all = catalog.entry("T").unwrap().expire_weapon_trigger_on;
        for condition in [
            ExpireWeaponCondition::EXPIRE,
            ExpireWeaponCondition::REMOVE,
            ExpireWeaponCondition::DEATH,
            ExpireWeaponCondition::DISCARD,
        ] {
            assert!(all.contains(condition));
        }
        catalog.apply_pass(&IniFile::from_str("[T]\nExpireWeapon.TriggerOn=death\n"));
        catalog.apply_pass(&IniFile::from_str(
            "[T]\nExpireWeapon.TriggerOn=death,bogus\n",
        ));
        assert_eq!(
            catalog.entry("T").unwrap().expire_weapon_trigger_on,
            ExpireWeaponCondition::DEATH
        );
    }

    /// `[General]` defaults match the documented values.
    #[test]
    fn globals_default_to_the_documented_switches() {
        let globals = AttachEffectGlobals::from_ini(&IniFile::from_str("[General]\n"));
        assert!(globals.discard_sequences_immediate);
        assert!(globals.discard_consider_harvesting_as_stationary);
        assert!(!globals.discard_move_based_on_destination);
        assert!(!globals.open_topped_use_transport_range_modifiers);
        assert!(!globals.open_topped_check_transport_disable_weapons);
        assert!(!globals.replace_longer_duration);
        assert!(!globals.attach_on_owner_change);

        // The `[General]` key is the plural `AttachEffects.AttachOnOwnerChange`
        // (`RulesExt::Body.cpp:595`); the singular spelling is the HouseType
        // key (`HouseTypeExt::Body.cpp:23`) and must not feed the global.
        let plural = AttachEffectGlobals::from_ini(&IniFile::from_str(
            "[General]\nAttachEffects.AttachOnOwnerChange=yes\n",
        ));
        assert!(plural.attach_on_owner_change);
        let singular = AttachEffectGlobals::from_ini(&IniFile::from_str(
            "[General]\nAttachEffect.AttachOnOwnerChange=yes\n",
        ));
        assert!(!singular.attach_on_owner_change);
    }
}
