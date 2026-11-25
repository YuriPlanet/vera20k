//! The reserved external-control seam: a versioned observation batch and a
//! batched action answer.
//!
//! **Nothing here transports anything.** There is no socket, no shared memory,
//! no `net` dependency and no scheduling: this module fixes the *contract* an
//! out-of-process controller would speak, so that wiring one later changes no
//! other module. [`PolicySource`] is the place a learned policy or a remote
//! script would plug in, beside [`super::brain::Brain`] rather than inside it.
//!
//! The shape is chosen for machine learning:
//!
//! - one **fixed-length integer tensor** per frame, so a batch is a plain array
//!   with no parsing and no allocation per feature;
//! - rows ordered by stable entity id, so row `r` names the same object on both
//!   sides of the wire;
//! - one **version** covering the whole layout. A version change is a breaking
//!   change: a controller pinned to an older version must be rejected rather
//!   than adapted, because silently shifting a feature column would corrupt a
//!   trained model;
//! - actions as **slots** — `(row, goal, target row, cell)` — so a policy can
//!   answer for many entities in one call instead of one call per unit.
//!
//! Determinism: a batch is built from the same integer state the in-process
//! brain reads, in the same order, so replaying a command stream reproduces the
//! batches that produced it. That is what makes an externally driven match
//! recordable at all.
//!
//! Residual: no batch is built unless a [`PolicySource`] is installed, so this
//! module costs a normal match nothing.

use crate::rules::ruleset::RuleSet;
use crate::sim::behavior::goal::Goal;
use crate::sim::control::observation::Observation;
use crate::sim::intern::InternedId;

/// The only layout version this build speaks.
pub const OBSERVATION_SCHEMA_VERSION: u32 = 1;

/// Global slots, in order.
pub const GLOBAL_SLOTS: usize = 6;
/// Per-row feature slots, in order.
pub const ENTITY_SLOTS: usize = 8;

/// The global features and their meaning, by slot.
pub const GLOBAL_FEATURES: [&str; GLOBAL_SLOTS] = [
    "frame",
    "credits",
    "power_output",
    "power_drain",
    "house_index",
    "own_entity_count",
];

/// The per-entity features and their meaning, by slot.
pub const ENTITY_FEATURES: [&str; ENTITY_SLOTS] = [
    "entity_id",
    "type_index",
    "cell_x",
    "cell_y",
    "health",
    "health_max",
    "mission",
    "orderable",
];

/// What a controller must expect of a batch: borrowed from this build, never
/// negotiated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObservationSchema {
    pub version: u32,
    pub globals: usize,
    pub entity_features: usize,
    pub global_features: &'static [&'static str],
    pub entity_feature_names: &'static [&'static str],
}

impl ObservationSchema {
    pub const fn current() -> Self {
        Self {
            version: OBSERVATION_SCHEMA_VERSION,
            globals: GLOBAL_SLOTS,
            entity_features: ENTITY_SLOTS,
            global_features: &GLOBAL_FEATURES,
            entity_feature_names: &ENTITY_FEATURES,
        }
    }
}

/// One frame, as a controller sees it: row-major, fixed length, integers only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObservationBatch {
    pub version: u32,
    /// `GLOBAL_SLOTS` values.
    pub globals: Vec<i64>,
    /// `rows × ENTITY_SLOTS` values, row-major, rows in stable id order.
    pub entities: Vec<i64>,
}

impl ObservationBatch {
    /// How many entity rows the batch carries.
    pub fn rows(&self) -> usize {
        self.entities.len() / ENTITY_SLOTS
    }

    /// One feature of one row. `None` when the row or slot is out of range.
    pub fn feature(&self, row: usize, slot: usize) -> Option<i64> {
        if slot >= ENTITY_SLOTS {
            return None;
        }
        self.entities.get(row * ENTITY_SLOTS + slot).copied()
    }

    /// The entity id of a row, which is also the id an action names.
    pub fn row_entity(&self, row: usize) -> Option<u64> {
        u64::try_from(self.feature(row, 0)?).ok()
    }
}

/// Build the batch for one house, at this frame.
///
/// Rows follow the entity store's ascending stable-id order, so two builds from
/// the same state produce identical vectors.
pub fn batch_for(obs: &Observation<'_>, rules: &RuleSet) -> ObservationBatch {
    let own: Vec<_> = obs.own_entities().collect();

    let mut globals = Vec::with_capacity(GLOBAL_SLOTS);
    globals.push(i64::try_from(obs.tick()).unwrap_or(i64::MAX));
    globals.push(i64::from(obs.credits()));
    globals.push(i64::from(obs.power_output()));
    globals.push(i64::from(obs.power_drain()));
    globals.push(i64::from(obs.house().index()));
    globals.push(i64::try_from(own.len()).unwrap_or(i64::MAX));

    let mut entities = Vec::with_capacity(own.len() * ENTITY_SLOTS);
    for entity in &own {
        let max_health = obs.max_health(rules, entity.type_id).unwrap_or(0);
        entities.push(i64::try_from(entity.entity_id).unwrap_or(i64::MAX));
        entities.push(i64::from(entity.type_id.index()));
        entities.push(i64::from(entity.cell.0));
        entities.push(i64::from(entity.cell.1));
        entities.push(i64::from(entity.health));
        entities.push(i64::from(max_health));
        entities.push(i64::from(entity.mission.raw()));
        entities.push(i64::from(entity.orderable));
    }

    ObservationBatch {
        version: OBSERVATION_SCHEMA_VERSION,
        globals,
        entities,
    }
}

/// A row's type id, back from its `type_index` feature.
pub fn row_type(batch: &ObservationBatch, row: usize) -> Option<InternedId> {
    let index = u32::try_from(batch.feature(row, 1)?).ok()?;
    Some(InternedId::from_index(index))
}

/// One thing a controller asks for, by row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionSlot {
    /// The acting row in the batch the action answers.
    pub row: u32,
    pub goal: Goal,
    /// The target's row, for goals that name an object.
    pub target_row: Option<u32>,
    /// A cell, for goals that name one.
    pub cell: Option<(u16, u16)>,
}

impl ActionSlot {
    /// This slot as the goal the control layer applies, with rows resolved
    /// against the batch it answered. `None` when a row is out of range, which
    /// is a malformed answer rather than an empty one.
    pub fn resolve(self, batch: &ObservationBatch) -> Option<(u64, Goal)> {
        let actor = batch.row_entity(usize::try_from(self.row).ok()?)?;
        let target = match self.target_row {
            Some(row) => batch.row_entity(usize::try_from(row).ok()?)?,
            None => 0,
        };
        let cell = self.cell.unwrap_or((0, 0));
        let goal = match self.goal {
            Goal::Attack { .. } => Goal::Attack { target },
            Goal::Capture { .. } => Goal::Capture { target },
            Goal::Enter { .. } => Goal::Enter { target },
            Goal::Grind { .. } => Goal::Grind { target },
            Goal::MoveToObject { .. } => Goal::MoveToObject { target },
            Goal::AttackCell { .. } => Goal::AttackCell { cell },
            Goal::AttackMove { .. } => Goal::AttackMove { cell },
            Goal::Move { .. } => Goal::Move { cell },
            Goal::GuardAt { .. } => Goal::GuardAt { cell },
            Goal::Harvest { .. } => Goal::Harvest { cell },
            other => other,
        };
        Some((actor, goal))
    }
}

/// An out-of-process decision source.
///
/// Implementations are expected to be pure functions of the batch: the caller
/// replays the same batches and compares the action vectors.
pub trait PolicySource {
    fn schema(&self) -> ObservationSchema;

    fn act(&mut self, batch: &ObservationBatch) -> Vec<ActionSlot>;
}
