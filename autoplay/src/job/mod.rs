//! Jobs: the only thing a unit can be given, and the one place that decides
//! which unit gets which.
//!
//! A manager proposes; the assignment gives each unit at most one job per term.
//! Ordering is deterministic and integer-keyed, and a unit with no job gets a
//! default one, so "nobody owns it" cannot happen.

use std::collections::BTreeMap;

use crate::registry::JobKind;

/// What a manager asks the assignment for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    pub kind: JobKind,
    /// The object or cell the job works on.
    pub target: Target,
    /// Larger runs first. Computed from the manager's state, never a constant
    /// per kind.
    pub priority: i64,
    /// The unit this job names. `None` is a demand the assignment satisfies
    /// with the nearest eligible unit; `Some` is a job for one object only.
    pub actor: Option<u64>,
}

/// What a job points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Object(u64),
    Cell((u16, u16)),
    None,
}

/// One unit's job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub kind: JobKind,
    pub target: Target,
    pub priority: i64,
    /// The term it was given in, so a job that has stood too long can be
    /// re-decided instead of holding a unit forever.
    pub given: u64,
}

/// What happened to a job, for the trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Given,
    Replaced,
    Refused,
    Done,
}

/// The jobs each unit holds, keyed by stable id.
#[derive(Debug, Default)]
pub struct Assignments {
    jobs: BTreeMap<u64, Job>,
}

impl Assignments {
    pub fn get(&self, unit: u64) -> Option<&Job> {
        self.jobs.get(&unit)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&u64, &Job)> {
        self.jobs.iter()
    }

    pub fn len(&self) -> usize {
        self.jobs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }

    /// Give this unit a job. Returns the job it replaced, if any.
    pub fn give(&mut self, unit: u64, job: Job) -> Option<Job> {
        self.jobs.insert(unit, job)
    }

    pub fn take(&mut self, unit: u64) -> Option<Job> {
        self.jobs.remove(&unit)
    }

    /// Forget units that no longer exist.
    pub fn retain(&mut self, alive: &dyn Fn(u64) -> bool) {
        self.jobs.retain(|unit, _| alive(*unit));
    }
}
