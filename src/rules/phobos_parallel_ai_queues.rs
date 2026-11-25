//! Phobos extended `ForbidParallelAIQueues` controls.
//!
//! `[GlobalControls] AllowParallelAIQueues=` gates AI factories producing the
//! same category in parallel. Phobos reads it into its own `RulesExt` with a
//! constructor default of true (`src/Ext/Rules/Body.h`), and Ares documents
//! `=no` as the switch that stops AI cloning, so the gate defaults on. Phobos
//! adds per-category and per-TechnoType forbids that only narrow it; with the
//! gate off they do nothing. With `=no` an Ares game produces one factory
//! building per category at a time (`RuleSet::serial_ai_queues_act`); any
//! other game keeps stock parallel production.
//!
//! They play only when Phobos does (`RuleSet::phobos`): a
//! `ForbidParallelAIQueues` key turns Phobos on (`rules::phobos_switch`);
//! `AllowParallelAIQueues`, which Ares reads too, turns neither extension
//! on.
//!
//! Sources: `RulesExt::ForbidParallelAIQueues_*` reads in
//! `src/Ext/Rules/Body.cpp`, `TechnoTypeExt::ForbidParallelAIQueues` in
//! `src/Ext/TechnoType/Body.cpp`, and the hooks in
//! `src/Ext/Building/Hooks.Production.cpp`.

use crate::rules::ini_parser::IniFile;
use crate::rules::object_type::ObjectCategory;
use crate::rules::phobos_switch::phobos_read;

/// Ares-Phobos parallel AI queue switches: the gate defaults on, every forbid
/// off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ParallelAiQueueRules {
    allow: bool,
    forbid_infantry: bool,
    forbid_vehicle: bool,
    forbid_navy: bool,
    forbid_aircraft: bool,
    forbid_building: bool,
}

impl Default for ParallelAiQueueRules {
    fn default() -> Self {
        Self {
            allow: true,
            forbid_infantry: false,
            forbid_vehicle: false,
            forbid_navy: false,
            forbid_aircraft: false,
            forbid_building: false,
        }
    }
}

impl ParallelAiQueueRules {
    pub fn from_ini(ini: &IniFile) -> Self {
        let defaults = Self::default();
        let Some(controls) = ini.section("GlobalControls") else {
            return defaults;
        };
        let forbid = |key: &str| phobos_read(|| controls.read_bool(key, false));
        Self {
            // Read as a stock key: both extensions read it.
            allow: controls.read_bool("AllowParallelAIQueues", defaults.allow),
            forbid_infantry: forbid("ForbidParallelAIQueues.Infantry"),
            forbid_vehicle: forbid("ForbidParallelAIQueues.Vehicle"),
            forbid_navy: forbid("ForbidParallelAIQueues.Navy"),
            forbid_aircraft: forbid("ForbidParallelAIQueues.Aircraft"),
            forbid_building: forbid("ForbidParallelAIQueues.Building"),
        }
    }

    /// The `AllowParallelAIQueues=` gate: on, the Phobos restrictions may
    /// act; off, Ares' serial queues do.
    pub const fn allows_parallel(&self) -> bool {
        self.allow
    }

    /// The category-wide forbid (`ForbidParallelAIQueues.<Category>=`).
    pub const fn forbids_category(&self, category: ObjectCategory, naval: bool) -> bool {
        match category {
            ObjectCategory::Infantry => self.forbid_infantry,
            ObjectCategory::Vehicle if naval => self.forbid_navy,
            ObjectCategory::Vehicle => self.forbid_vehicle,
            ObjectCategory::Aircraft => self.forbid_aircraft,
            ObjectCategory::Building => self.forbid_building,
        }
    }

    pub(crate) fn hash_into<H: std::hash::Hasher>(&self, hasher: &mut H) {
        use std::hash::Hash;
        b"phobos-parallel-ai-queues-v1".hash(hasher);
        self.allow.hash(hasher);
        self.forbid_infantry.hash(hasher);
        self.forbid_vehicle.hash(hasher);
        self.forbid_navy.hash(hasher);
        self.forbid_aircraft.hash(hasher);
        self.forbid_building.hash(hasher);
    }
}
