//! Modifiers (RFC 0001 section 6c, appendix 7.2 and 7.3): timed changes a
//! module puts on an object's modifiable property.
//!
//! [`ModifierStore`] is the one owner: it holds every modifier by handle,
//! expires them in the module phase, drops those of a removed object, and is
//! saved and hashed with the modules. Each object with modifiers carries
//! derived stats and counts by origin and group in [`ModifierTerms`] in
//! `Seldom`. The store rebuilds them on addition, removal, expiry and load.
//! Objects and properties without modifier terms keep their original stats.
//!
//! Wired properties and where they compose:
//! - `FirepowerMultiplier`: the Phobos `TechnoClass_FirepowerMultiplier`
//!   stage of FireAt's damage build (`0x006FE352`), which also feeds
//!   EstimateDamage and Phobos' bullet-init capture
//!   (`world_fire::firepower_stages`).
//! - `ArmorMultiplier`: the Phobos receive-damage armor divide
//!   (`TechnoClass_ReceiveDamage_ArmorMultiplier`, `0x00701966`), through
//!   `receiver_call::receiver_divisors` and EstimateDamage's divisors. As in
//!   Phobos it divides incoming damage; the warhead filters stay with Phobos'
//!   own effect types.
//! - `SpeedMultiplier`: the Phobos `FootClass_GetMovementSpeed_SpeedMultiplier`
//!   stage (`0x4DB218`) every mover's GetCurrentSpeed applies.
//! - `ROFMultiplier`: the Phobos `TechnoClass_RearmDelay_ROF` factor
//!   (`0x006FD0B5`) GetROF multiplies `ROF=` by (`rof::RofQuery::ae_rof`).
//!   Phobos' `ROFMultiplier.ApplyOnCurrentTimer` is not applied: a modifier
//!   changes the next rearm, not a running one.
//! - `WeaponRangeMultiplier` and `WeaponRangeExtra` (leptons): Phobos'
//!   `WeaponTypeExt::GetRangeWithModifiers`
//!   (`attach_effect::range_with_attach_effects`), which every range reader
//!   asks: the multiplier after the effects' multipliers, the extra beside
//!   their summed extra range, then the floor at 0. The `-512` always-in-range
//!   sentinel is left alone.
//! - `CritMultiplier` and `CritExtraChance` (Phobos builds only, where crits
//!   exist): `WarheadTypeExt::GetCritChance`'s AttachEffect fold
//!   (`combat::crit`), the multiplier with the effects' multipliers and the
//!   extra chance beside theirs.
//! - `Unkillable` (Phobos builds only): the Phobos ReceiveDamage wrapper's
//!   Unkillable arm (`attach_effect::combat::apply_unkillable_clamp`).
//! - `Cloakable` and `ForceDecloak`: Phobos' `IsReadyToCloak_Wrapper` and
//!   `ShouldNotBeCloaked_Wrapper` terms (`world::techno_ai_cloak`). The base
//!   value is the Phobos effects' flag, so an `Override` of 0 does not take
//!   away the type's own `Cloakable=`.
//! - `DisableWeapons`: GetFireError answers Rearm at the Phobos hook
//!   `0x006FC0C5` (`combat::fire_error`). Phobos' own effect flag stays
//!   unwired there (its residual in `attach_effect::stats`), so it is not the
//!   base value; the open-topped transport check (`0x006FC5C7`) is not made.
//!
//! Each stage starts from the Phobos AttachEffect product (1.0 without
//! Phobos), scales it by `1 + sum of additions` (an additive property:
//! adds the sum), multiplies in each module multiplier in handle order, as
//! Phobos multiplies in one more effect type's factor, and the most recent
//! override replaces the result. A module `Multiply` therefore lands
//! exactly where an equal Phobos effect does. The result is clamped to
//! `[0, 32768]` (armor: at least 2^-16, so the divide never reaches zero;
//! flags to `[0, 1]`), which only absurd module values reach. `Multiply`
//! operands are `fixed`; `Add` and `Override` operands are in the
//! property's own unit (`fixed`, leptons or a flag).
//!
//! Handles share the objects' creation counter (`allocate_stable_id`), as
//! the appendix's references ask, so a modifier handle is a kind-0
//! reference that never names an object.
//!
//! Modifier events (appendix 4.3), bound to the target object:
//! `ModifierStart` after a modifier is added (`value` its property, `other`
//! its source), `ModifierStacks` when that addition raised the number of the
//! target's modifiers in the group (any origin; `value` the new number), and
//! `ModifierExpire` after expiry, `unmodify`, `RemoveModifiers` or a
//! same-group replacement. A removed object's modifiers go without events.
//!
//! Residuals: houses take no modifiers (`PowerOutage` would have to compose
//! with the power owner's blackout timer, which several owners read);
//! `CritMultiplier`, `CritExtraChance` and `Unkillable` answer `Unsupported`
//! in a build without Phobos, whose owners are not compiled there.

use std::collections::BTreeMap;

use vera20k_wasm_ext::triggers::{
    EVENT_MODIFIER_EXPIRE, EVENT_MODIFIER_STACKS, EVENT_MODIFIER_START,
};

use super::{ModuleEvent, ModuleSource};
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::world::{FrameEffects, Simulation};

/// `ModifierOp` (appendix 8.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ModifierOp {
    Add,
    Multiply,
    Override,
}

impl ModifierOp {
    /// From the ABI value, which the sandbox checked to be 0..=2.
    pub(super) const fn from_abi(op: i32) -> Self {
        match op {
            0 => Self::Add,
            1 => Self::Multiply,
            _ => Self::Override,
        }
    }

    const fn abi(self) -> i64 {
        match self {
            Self::Add => 0,
            Self::Multiply => 1,
            Self::Override => 2,
        }
    }
}

/// The modifiable properties (appendix 7.2) wired into engine owners. New
/// variants go at the end: the discriminant is checksum data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Stat {
    Firepower,
    Armor,
    Speed,
    Rof,
    RangeMultiplier,
    RangeExtra,
    CritMultiplier,
    CritExtraChance,
    Unkillable,
    Cloakable,
    ForceDecloak,
    DisableWeapons,
}

/// How a property's value reads and composes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    /// A `fixed` factor, base 1.0: additions scale it.
    Factor,
    /// A `fixed` amount, base 0: additions add.
    Fixed,
    /// Leptons, base 0.
    Leptons,
    /// A flag, 0 or 1.
    Flag,
}

impl Stat {
    const COUNT: usize = 12;

    pub(super) fn from_property(name: &str) -> Option<Self> {
        Some(match name {
            "FirepowerMultiplier" => Self::Firepower,
            "ArmorMultiplier" => Self::Armor,
            "SpeedMultiplier" => Self::Speed,
            "ROFMultiplier" => Self::Rof,
            "WeaponRangeMultiplier" => Self::RangeMultiplier,
            "WeaponRangeExtra" => Self::RangeExtra,
            "CritMultiplier" => Self::CritMultiplier,
            "CritExtraChance" => Self::CritExtraChance,
            "Unkillable" => Self::Unkillable,
            "Cloakable" => Self::Cloakable,
            "ForceDecloak" => Self::ForceDecloak,
            "DisableWeapons" => Self::DisableWeapons,
            _ => return None,
        })
    }

    /// A property this build's owners read ([`Self::from_property`] filtered).
    pub(super) fn wired_property(name: &str) -> Option<Self> {
        Self::from_property(name).filter(|stat| stat.wired())
    }

    pub(super) const fn property(self) -> &'static str {
        match self {
            Self::Firepower => "FirepowerMultiplier",
            Self::Armor => "ArmorMultiplier",
            Self::Speed => "SpeedMultiplier",
            Self::Rof => "ROFMultiplier",
            Self::RangeMultiplier => "WeaponRangeMultiplier",
            Self::RangeExtra => "WeaponRangeExtra",
            Self::CritMultiplier => "CritMultiplier",
            Self::CritExtraChance => "CritExtraChance",
            Self::Unkillable => "Unkillable",
            Self::Cloakable => "Cloakable",
            Self::ForceDecloak => "ForceDecloak",
            Self::DisableWeapons => "DisableWeapons",
        }
    }

    /// Whether an owner reads this property in this build: crits and
    /// Phobos' Unkillable arm exist only where Phobos is compiled.
    #[cfg(feature = "phobos")]
    const fn wired(self) -> bool {
        true
    }

    #[cfg(not(feature = "phobos"))]
    const fn wired(self) -> bool {
        !matches!(
            self,
            Self::CritMultiplier | Self::CritExtraChance | Self::Unkillable
        )
    }

    const fn unit(self) -> Unit {
        match self {
            Self::Firepower
            | Self::Armor
            | Self::Speed
            | Self::Rof
            | Self::RangeMultiplier
            | Self::CritMultiplier => Unit::Factor,
            Self::CritExtraChance => Unit::Fixed,
            Self::RangeExtra => Unit::Leptons,
            Self::Unkillable | Self::Cloakable | Self::ForceDecloak | Self::DisableWeapons => {
                Unit::Flag
            }
        }
    }

    /// One unit of the property in its raw module value.
    fn scale(self) -> f64 {
        match self.unit() {
            Unit::Factor | Unit::Fixed => FIXED_ONE,
            Unit::Leptons | Unit::Flag => 1.0,
        }
    }

    /// The clamp on the composed value.
    fn range(self) -> (f64, f64) {
        match (self, self.unit()) {
            (Self::Armor, _) => (1.0 / FIXED_ONE, 32768.0),
            (_, Unit::Factor) => (0.0, 32768.0),
            (_, Unit::Fixed) => (-32768.0, 32768.0),
            (_, Unit::Leptons) => (-16_777_216.0, 16_777_216.0),
            (_, Unit::Flag) => (0.0, 1.0),
        }
    }

    /// The Phobos AttachEffect value this stage starts from. Range and crit
    /// fold per weapon or warhead at their sites, so they start from the
    /// identity here.
    fn phobos(self, entity: &GameEntity) -> f64 {
        let flag = |on: bool| if on { 1.0 } else { 0.0 };
        match self {
            #[cfg(feature = "phobos")]
            Self::Firepower => entity.phobos_stat_multipliers().firepower,
            #[cfg(not(feature = "phobos"))]
            Self::Firepower => 1.0,
            Self::Armor => entity.phobos_armor_multiplier(),
            Self::Speed => entity.phobos_speed_multiplier(),
            Self::Rof => entity.phobos_rof_multiplier(),
            Self::RangeMultiplier | Self::CritMultiplier => 1.0,
            Self::RangeExtra | Self::CritExtraChance | Self::DisableWeapons => 0.0,
            Self::Unkillable => flag(entity.phobos_unkillable()),
            Self::Cloakable => flag(entity.phobos_cloakable()),
            Self::ForceDecloak => flag(entity.phobos_force_decloak()),
        }
    }
}

/// `1.0` in a `fixed` value (raw `I16F16`).
const FIXED_ONE: f64 = 65536.0;

/// Most modifiers one module origin may hold at once (open question 13).
pub const MAX_MODIFIERS_PER_ORIGIN: usize = 4096;

/// One modifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Modifier {
    /// The modified object's creation serial.
    pub target: u64,
    pub stat: Stat,
    pub op: ModifierOp,
    /// As supplied: raw `fixed` for the multipliers.
    pub value: i64,
    /// The frame whose module phase removes it; `None` for unlimited.
    pub expires: Option<u32>,
    /// Source object reference, or 0.
    pub source: i64,
    /// Exclusion group, 0 for none.
    pub group: i32,
    pub origin: ModuleSource,
}

/// Every modifier, by handle (insertion order: handles only grow).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ModifierStore {
    modifiers: BTreeMap<u64, Modifier>,
}

/// One stat's module terms, folded onto the Phobos value.
#[derive(Debug, Clone, PartialEq)]
struct Terms {
    /// The sum of additions, in the property's unit.
    added: f64,
    /// In handle order.
    multipliers: Vec<f64>,
    /// The most recent override.
    replace: Option<f64>,
}

impl Terms {
    const NONE: Self = Self {
        added: 0.0,
        multipliers: Vec::new(),
        replace: None,
    };

    fn is_none(&self) -> bool {
        *self == Self::NONE
    }

    fn apply(&self, stat: Stat, base: f64) -> f64 {
        let value = match self.replace {
            Some(value) => value,
            None => {
                let start = match stat.unit() {
                    Unit::Factor => base * (1.0 + self.added),
                    Unit::Fixed | Unit::Leptons | Unit::Flag => base + self.added,
                };
                self.multipliers
                    .iter()
                    .fold(start, |value, factor| value * factor)
            }
        };
        let (floor, ceiling) = stat.range();
        value.clamp(floor, ceiling)
    }
}

/// The derived per-object cache the engine owners read (see the module
/// documentation). Never saved: [`ModifierStore::refresh`] rebuilds it.
#[derive(Debug, Clone, PartialEq)]
pub struct ModifierTerms {
    stats: [Terms; Stat::COUNT],
    group_counts: BTreeMap<(ModuleSource, i32), usize>,
}

impl ModifierTerms {
    fn stat(&self, stat: Stat) -> &Terms {
        &self.stats[stat as usize]
    }

    /// `stat`'s terms on `base`; `None` when no modifier touches it.
    fn applied(&self, stat: Stat, base: f64) -> Option<f64> {
        let terms = self.stat(stat);
        (!terms.is_none()).then(|| terms.apply(stat, base))
    }
}

impl GameEntity {
    fn module_terms(&self) -> Option<&ModifierTerms> {
        self.seldom.module_modifiers.as_deref()
    }

    /// Active modifiers on this object from `origin` in `group`,
    /// including modifiers whose terms leave the stat unchanged.
    pub(super) fn module_modifier_count(&self, origin: ModuleSource, group: i32) -> usize {
        self.module_terms()
            .and_then(|terms| terms.group_counts.get(&(origin, group)))
            .copied()
            .unwrap_or(0)
    }

    /// `stage` (the Phobos value for `stat`) with this object's module
    /// modifiers applied; `stage` itself when none touches `stat`.
    pub(crate) fn with_module_modifiers(&self, stat: Stat, stage: f64) -> f64 {
        self.module_terms()
            .and_then(|terms| terms.applied(stat, stage))
            .unwrap_or(stage)
    }

    /// A flag stage with this object's module modifiers applied.
    fn with_module_flag(&self, stat: Stat, phobos: bool) -> bool {
        match self.module_terms() {
            None => phobos,
            Some(terms) => terms
                .applied(stat, if phobos { 1.0 } else { 0.0 })
                .map_or(phobos, |value| value != 0.0),
        }
    }

    /// The firepower stage of FireAt's damage build: Phobos' AttachEffect
    /// product and the module modifiers.
    pub(crate) fn effect_firepower_multiplier(&self) -> f64 {
        self.with_module_modifiers(Stat::Firepower, Stat::Firepower.phobos(self))
    }

    /// The speed stage of GetCurrentSpeed: Phobos' AttachEffect product and
    /// the module modifiers.
    pub(crate) fn effect_speed_multiplier(&self) -> f64 {
        self.with_module_modifiers(Stat::Speed, self.phobos_speed_multiplier())
    }

    /// GetROF's `ROF=` factor (`0x006FD0B5`): Phobos' AttachEffect product
    /// and the module modifiers.
    pub(crate) fn effect_rof_multiplier(&self) -> f64 {
        self.with_module_modifiers(Stat::Rof, self.phobos_rof_multiplier())
    }

    /// `Cloakable` from the active AttachEffects and the module modifiers.
    pub(crate) fn effect_cloakable(&self) -> bool {
        self.with_module_flag(Stat::Cloakable, self.phobos_cloakable())
    }

    /// `ForceDecloak` from the active AttachEffects and the module
    /// modifiers.
    pub(crate) fn effect_force_decloak(&self) -> bool {
        self.with_module_flag(Stat::ForceDecloak, self.phobos_force_decloak())
    }

    /// `Unkillable` from the active AttachEffects and the module modifiers.
    #[cfg(feature = "phobos")]
    pub(crate) fn effect_unkillable(&self) -> bool {
        self.with_module_flag(Stat::Unkillable, self.phobos_unkillable())
    }

    /// A module `DisableWeapons` modifier holds.
    pub(crate) fn module_weapons_disabled(&self) -> bool {
        self.with_module_flag(Stat::DisableWeapons, false)
    }

    /// The module range terms: the multiplier and the extra leptons, or
    /// `None` when no modifier touches either.
    pub(crate) fn module_range_terms(&self) -> Option<(f64, i32)> {
        let terms = self.module_terms()?;
        let multiplier = terms.applied(Stat::RangeMultiplier, 1.0);
        let extra = terms.applied(Stat::RangeExtra, 0.0);
        if multiplier.is_none() && extra.is_none() {
            return None;
        }
        Some((
            multiplier.unwrap_or(1.0),
            extra.map_or(0, |extra| extra as i32),
        ))
    }

    /// The module crit terms: the chance multiplier and the extra chance,
    /// or `None` when no modifier touches either.
    #[cfg(feature = "phobos")]
    pub(crate) fn module_crit_terms(&self) -> Option<(f64, f64)> {
        let terms = self.module_terms()?;
        let multiplier = terms.applied(Stat::CritMultiplier, 1.0);
        let extra = terms.applied(Stat::CritExtraChance, 0.0);
        if multiplier.is_none() && extra.is_none() {
            return None;
        }
        Some((multiplier.unwrap_or(1.0), extra.unwrap_or(0.0)))
    }

    /// A wired property's effective value, raw: `fixed` for factors and
    /// amounts, leptons, or 0/1 for a flag.
    pub(super) fn effective_stat(&self, stat: Stat) -> i64 {
        let value = self.with_module_modifiers(stat, stat.phobos(self));
        (value * stat.scale()).round() as i64
    }
}

/// Modifiers a store operation took away, with their handles.
pub(super) type Removed = Vec<(u64, Modifier)>;

impl ModifierStore {
    pub fn is_empty(&self) -> bool {
        self.modifiers.is_empty()
    }

    pub fn get(&self, handle: u64) -> Option<&Modifier> {
        self.modifiers.get(&handle)
    }

    /// Handles from `after` on, in insertion order.
    pub(super) fn handles_from(&self, after: u64) -> impl Iterator<Item = u64> + '_ {
        self.modifiers.range(after..).map(|(handle, _)| *handle)
    }

    /// Every modifier with its handle, in insertion order.
    pub(super) fn iter(&self) -> impl Iterator<Item = (u64, &Modifier)> + '_ {
        self.modifiers.iter().map(|(handle, m)| (*handle, m))
    }

    /// How many modifiers `target` holds in `group`, of any origin.
    pub(super) fn group_count(&self, target: u64, group: i32) -> usize {
        self.modifiers
            .values()
            .filter(|m| m.target == target && m.group == group)
            .count()
    }

    /// Takes the modifiers `keep` refuses out of the store, in handle order.
    fn take(&mut self, keep: impl Fn(&Modifier) -> bool) -> Removed {
        let handles: Vec<u64> = self
            .modifiers
            .iter()
            .filter(|&(_, m)| !keep(m))
            .map(|(handle, _)| *handle)
            .collect();
        handles
            .into_iter()
            .filter_map(|handle| self.modifiers.remove(&handle).map(|m| (handle, m)))
            .collect()
    }

    /// `RemoveModifiers` (appendix 5.3): drops the modifiers `origin` put on
    /// `target` in `group`, only those of `stat` when given.
    pub(super) fn remove_group(
        &mut self,
        entities: &mut EntityStore,
        target: u64,
        origin: ModuleSource,
        group: i32,
        stat: Option<Stat>,
    ) -> Removed {
        let removed = self.take(|m| {
            !(m.target == target
                && m.origin == origin
                && m.group == group
                && stat.is_none_or(|stat| m.stat == stat))
        });
        if !removed.is_empty() {
            self.refresh(entities, target);
        }
        removed
    }

    /// Adds `modifier` as `handle`, replacing the member of its group that
    /// the same origin put on the same object. `None`, and nothing changed,
    /// when the origin holds its quota; else the replaced modifiers.
    pub(super) fn insert(
        &mut self,
        entities: &mut EntityStore,
        handle: u64,
        modifier: Modifier,
    ) -> Option<Removed> {
        let (origin, target, group) = (modifier.origin, modifier.target, modifier.group);
        let replaces = move |m: &Modifier| {
            group != 0 && m.origin == origin && m.target == target && m.group == group
        };
        let held = self
            .modifiers
            .values()
            .filter(|&m| m.origin == origin && !replaces(m))
            .count();
        if held >= MAX_MODIFIERS_PER_ORIGIN {
            return None;
        }
        let replaced = self.take(|m| !replaces(m));
        self.modifiers.insert(handle, modifier);
        self.refresh(entities, target);
        Some(replaced)
    }

    /// Removes one modifier; `None` when none has that handle.
    pub(super) fn remove(&mut self, entities: &mut EntityStore, handle: u64) -> Option<Modifier> {
        let removed = self.modifiers.remove(&handle)?;
        self.refresh(entities, removed.target);
        Some(removed)
    }

    /// Removes the modifiers whose time is up at `frame`, in handle order.
    pub(super) fn expire(&mut self, entities: &mut EntityStore, frame: u32) -> Removed {
        let removed = self.take(|m| m.expires.is_none_or(|at| at > frame));
        let mut touched: Vec<u64> = removed.iter().map(|(_, m)| m.target).collect();
        touched.sort_unstable();
        touched.dedup();
        for target in touched {
            self.refresh(entities, target);
        }
        removed
    }

    /// Drops a removed object's modifiers.
    pub(super) fn forget_object(&mut self, target: u64) {
        self.modifiers.retain(|_, m| m.target != target);
    }

    /// Rebuilds every object's cache, after a load.
    pub(super) fn refresh_all(&self, entities: &mut EntityStore) {
        let mut targets: Vec<u64> = self.modifiers.values().map(|m| m.target).collect();
        targets.sort_unstable();
        targets.dedup();
        for target in targets {
            self.refresh(entities, target);
        }
    }

    /// Recomputes `target`'s cache from its modifiers.
    fn refresh(&self, entities: &mut EntityStore, target: u64) {
        let Some(entity) = entities.get_mut(target) else {
            return;
        };
        let mut stats: [Terms; Stat::COUNT] = std::array::from_fn(|_| Terms::NONE);
        let mut added = [0i64; Stat::COUNT];
        let mut group_counts = BTreeMap::new();
        for modifier in self.modifiers.values().filter(|m| m.target == target) {
            *group_counts
                .entry((modifier.origin, modifier.group))
                .or_default() += 1;
            let slot = modifier.stat as usize;
            let own = modifier.value as f64 / modifier.stat.scale();
            match modifier.op {
                ModifierOp::Add => added[slot] = added[slot].saturating_add(modifier.value),
                ModifierOp::Multiply => stats[slot]
                    .multipliers
                    .push(modifier.value as f64 / FIXED_ONE),
                ModifierOp::Override => stats[slot].replace = Some(own),
            }
        }
        for ((terms, added), scale) in stats.iter_mut().zip(added).zip(ALL_STATS.map(Stat::scale)) {
            terms.added = added as f64 / scale;
        }
        entity.seldom.module_modifiers = (!group_counts.is_empty()).then(|| {
            Box::new(ModifierTerms {
                stats,
                group_counts,
            })
        });
    }
}

/// Every [`Stat`], in discriminant order.
const ALL_STATS: [Stat; Stat::COUNT] = [
    Stat::Firepower,
    Stat::Armor,
    Stat::Speed,
    Stat::Rof,
    Stat::RangeMultiplier,
    Stat::RangeExtra,
    Stat::CritMultiplier,
    Stat::CritExtraChance,
    Stat::Unkillable,
    Stat::Cloakable,
    Stat::ForceDecloak,
    Stat::DisableWeapons,
];

impl Modifier {
    /// A modifier's property (appendix 7.3), at `frame`.
    pub(super) fn property(&self, name: &str, frame: u32) -> Option<i64> {
        Some(match name {
            "Target" => self.target as i64,
            "Property" => i64::from(vera20k_wasm_ext::property_id(self.stat.property())?),
            "Op" => self.op.abi(),
            "Value" => self.value,
            "Remaining" => self
                .expires
                .map_or(-1, |at| i64::from(at.saturating_sub(frame))),
            "Source" => self.source,
            "Group" => i64::from(self.group),
            "Origin" => self.origin as i64,
            _ => return None,
        })
    }

    /// Its event record on the target object: `modifier` the handle,
    /// `value` the property, `other` the source.
    fn event(&self, kind: i32, handle: u64) -> ModuleEvent {
        ModuleEvent {
            modifier: handle,
            other: u64::try_from(self.source).unwrap_or(0),
            value: vera20k_wasm_ext::property_id(self.stat.property()).map_or(-1, i64::from),
            ..ModuleEvent::new(kind, self.target)
        }
    }
}

impl Simulation {
    /// `ModifierExpire` for each modifier in `removed`, in handle order.
    pub(super) fn raise_modifiers_expired(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
        removed: &[(u64, Modifier)],
    ) {
        if !self.trigger_runtime.module_listens(EVENT_MODIFIER_EXPIRE) {
            return;
        }
        for (handle, modifier) in removed {
            let event = modifier.event(EVENT_MODIFIER_EXPIRE, *handle);
            self.raise_module_event(rules, frame_effects, event);
        }
    }

    /// `ModifierStart` for the modifier just added as `handle`, then
    /// `ModifierStacks` when its group on the target grew from `before`.
    pub(super) fn raise_modifier_started(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
        handle: u64,
        before: usize,
    ) {
        let Some(modifier) = self.ext_modules.modifiers.get(handle).cloned() else {
            return;
        };
        if self.trigger_runtime.module_listens(EVENT_MODIFIER_START) {
            let event = modifier.event(EVENT_MODIFIER_START, handle);
            self.raise_module_event(rules, frame_effects, event);
        }
        if modifier.group == 0 || !self.trigger_runtime.module_listens(EVENT_MODIFIER_STACKS) {
            return;
        }
        let count = self
            .ext_modules
            .modifiers
            .group_count(modifier.target, modifier.group);
        if count > before {
            let event = ModuleEvent {
                value: count as i64,
                ..modifier.event(EVENT_MODIFIER_STACKS, handle)
            };
            self.raise_module_event(rules, frame_effects, event);
        }
    }
}
