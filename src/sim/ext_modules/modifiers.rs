//! Modifiers (RFC 0001 section 6c, appendix 7.2 and 7.3): timed changes a
//! module puts on an object's modifiable property.
//!
//! [`ModifierStore`] is the one owner: it holds every modifier by handle,
//! expires them in the module phase, drops those of a removed object, and is
//! saved and hashed with the modules. Each object with modifiers on a wired
//! property carries a derived [`ModifierTerms`] in `Seldom`, rebuilt from the
//! store on every addition, removal, expiry and load; the engine owners read
//! only that cache, so an object without modifiers costs them one `None`
//! check and computes exactly what it computed before.
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
//!
//! Each stage starts from the Phobos AttachEffect product (1.0 without
//! Phobos), scales it by `1 + sum of additions`, multiplies in each module
//! multiplier in handle order, as Phobos multiplies in one more effect type's
//! factor, and the most recent override replaces the result. A module
//! `Multiply` therefore lands exactly where an equal Phobos effect does. The
//! result is clamped to `[0, 32768]` (armor: at least 2^-16, so the divide
//! never reaches zero), which only absurd module values reach.
//!
//! Handles share the objects' creation counter (`allocate_stable_id`), as
//! the appendix's references ask, so a modifier handle is a kind-0
//! reference that never names an object.

use std::collections::BTreeMap;

use super::ModuleSource;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;

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

/// The modifiable properties (appendix 7.2) wired into engine owners.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Stat {
    Firepower,
    Armor,
    Speed,
}

impl Stat {
    pub(super) fn from_property(name: &str) -> Option<Self> {
        Some(match name {
            "FirepowerMultiplier" => Self::Firepower,
            "ArmorMultiplier" => Self::Armor,
            "SpeedMultiplier" => Self::Speed,
            _ => return None,
        })
    }

    pub(super) const fn property(self) -> &'static str {
        match self {
            Self::Firepower => "FirepowerMultiplier",
            Self::Armor => "ArmorMultiplier",
            Self::Speed => "SpeedMultiplier",
        }
    }

    /// The Phobos AttachEffect product this stage starts from.
    fn phobos(self, entity: &GameEntity) -> f64 {
        match self {
            #[cfg(feature = "phobos")]
            Self::Firepower => entity.phobos_stat_multipliers().firepower,
            #[cfg(not(feature = "phobos"))]
            Self::Firepower => 1.0,
            Self::Armor => entity.phobos_armor_multiplier(),
            Self::Speed => entity.phobos_speed_multiplier(),
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

/// One stat's module terms, folded onto the Phobos product.
#[derive(Debug, Clone, PartialEq)]
struct Terms {
    /// `1 + sum of additions`.
    scale: f64,
    /// In handle order.
    multipliers: Vec<f64>,
    /// The most recent override.
    replace: Option<f64>,
}

impl Terms {
    const NONE: Self = Self {
        scale: 1.0,
        multipliers: Vec::new(),
        replace: None,
    };

    fn is_none(&self) -> bool {
        *self == Self::NONE
    }

    fn apply(&self, stat: Stat, phobos: f64) -> f64 {
        let value = match self.replace {
            Some(value) => value,
            None => self
                .multipliers
                .iter()
                .fold(phobos * self.scale, |value, factor| value * factor),
        };
        let floor = if stat == Stat::Armor {
            1.0 / FIXED_ONE
        } else {
            0.0
        };
        value.clamp(floor, 32768.0)
    }
}

/// The derived per-object cache the engine owners read (see the module
/// documentation). Never saved: [`ModifierStore::refresh`] rebuilds it.
#[derive(Debug, Clone, PartialEq)]
pub struct ModifierTerms {
    firepower: Terms,
    armor: Terms,
    speed: Terms,
}

impl ModifierTerms {
    fn stat(&self, stat: Stat) -> &Terms {
        match stat {
            Stat::Firepower => &self.firepower,
            Stat::Armor => &self.armor,
            Stat::Speed => &self.speed,
        }
    }
}

impl GameEntity {
    /// `stage` (the Phobos product for `stat`) with this object's module
    /// modifiers applied; `stage` itself when it has none.
    pub(crate) fn with_module_modifiers(&self, stat: Stat, stage: f64) -> f64 {
        match &self.seldom.module_modifiers {
            None => stage,
            Some(terms) => terms.stat(stat).apply(stat, stage),
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

    /// A wired property's effective value, as raw `fixed`.
    pub(super) fn effective_stat(&self, stat: Stat) -> i64 {
        let value = self.with_module_modifiers(stat, stat.phobos(self));
        (value * FIXED_ONE).round() as i64
    }
}

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

    /// Adds `modifier` as `handle`, first removing the member of its group
    /// that the same origin put on the same object. `false` when the origin
    /// holds its quota.
    pub(super) fn insert(
        &mut self,
        entities: &mut EntityStore,
        handle: u64,
        modifier: Modifier,
    ) -> bool {
        if modifier.group != 0 {
            self.modifiers.retain(|_, m| {
                !(m.origin == modifier.origin
                    && m.target == modifier.target
                    && m.group == modifier.group)
            });
        }
        let held = self
            .modifiers
            .values()
            .filter(|m| m.origin == modifier.origin)
            .count();
        if held >= MAX_MODIFIERS_PER_ORIGIN {
            return false;
        }
        let target = modifier.target;
        self.modifiers.insert(handle, modifier);
        self.refresh(entities, target);
        true
    }

    /// Removes one modifier; `false` when none has that handle.
    pub(super) fn remove(&mut self, entities: &mut EntityStore, handle: u64) -> bool {
        let Some(removed) = self.modifiers.remove(&handle) else {
            return false;
        };
        self.refresh(entities, removed.target);
        true
    }

    /// Removes the modifiers whose time is up at `frame`, in handle order.
    pub(super) fn expire(&mut self, entities: &mut EntityStore, frame: u32) {
        let mut touched = Vec::new();
        self.modifiers.retain(|_, m| {
            let live = m.expires.is_none_or(|at| at > frame);
            if !live {
                touched.push(m.target);
            }
            live
        });
        touched.dedup();
        for target in touched {
            self.refresh(entities, target);
        }
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
        let mut terms = ModifierTerms {
            firepower: Terms::NONE,
            armor: Terms::NONE,
            speed: Terms::NONE,
        };
        let mut added = [0i64; 3];
        for modifier in self.modifiers.values().filter(|m| m.target == target) {
            let slot = modifier.stat as usize;
            let stat = match modifier.stat {
                Stat::Firepower => &mut terms.firepower,
                Stat::Armor => &mut terms.armor,
                Stat::Speed => &mut terms.speed,
            };
            let value = modifier.value as f64 / FIXED_ONE;
            match modifier.op {
                ModifierOp::Add => added[slot] = added[slot].saturating_add(modifier.value),
                ModifierOp::Multiply => stat.multipliers.push(value),
                ModifierOp::Override => stat.replace = Some(value),
            }
        }
        for (stat, added) in [&mut terms.firepower, &mut terms.armor, &mut terms.speed]
            .into_iter()
            .zip(added)
        {
            stat.scale = 1.0 + added as f64 / FIXED_ONE;
        }
        let none = terms.firepower.is_none() && terms.armor.is_none() && terms.speed.is_none();
        entity.seldom.module_modifiers = (!none).then(|| Box::new(terms));
    }
}

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
}
