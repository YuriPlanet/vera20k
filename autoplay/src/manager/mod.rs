//! Managers: the only things allowed to ask for work.
//!
//! A manager reads the projection and proposes jobs. It never issues a command,
//! never touches another manager's state, and never picks the unit — the
//! assignment does that. One file per manager, registered in crate::registry.

pub mod abilities;
pub mod build;
pub mod economy;
pub mod military;

use vera20k_core::rules::ruleset::RuleSet;
use vera20k_core::rules::unit_role::RoleTable;
use vera20k_core::sim::control::intent::Intent;
use vera20k_core::sim::control::observation::Observation;

use crate::job::Proposal;
use crate::profile::Profiles;

/// What a manager may read. Read-only on purpose: a manager that could write
/// is a manager that decides who wins the unit.
pub struct Ctx<'a> {
    pub rules: &'a RuleSet,
    pub obs: &'a Observation<'a>,
    pub profiles: &'a Profiles,
    /// What every type is for, read off the rules' own weapon and warhead
    /// data by the engine's role model.
    pub roles: &'a RoleTable,
    pub tick: u64,
}

/// Everything the managers ask for in one term.
///
/// Two channels, because two kinds of work exist: a **job** is a unit doing
/// something, and an **intent** is a house doing something (queue a building,
/// place one). The assignment owns the first; the control layer owns the second
/// and admits it the same way.
#[derive(Debug, Default)]
pub struct Demands {
    pub jobs: Vec<Proposal>,
    pub intents: Vec<Intent>,
    /// Where the house's base is, as the manager that knows it says.
    ///
    /// Only one layer may own "where is home": the military manager reads
    /// the house's first structure, and the policy learns it from there.
    pub home: Option<(u16, u16)>,
}

/// What a manager may do in one term.
pub trait Manager {
    /// Propose what this manager wants done. Read-only over the projection.
    fn propose(&mut self, ctx: &Ctx<'_>, out: &mut Demands);
}
