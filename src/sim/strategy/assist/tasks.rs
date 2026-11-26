//! Cross-frame specialist ownership, separate from the command debounce table.
//!
//! A Move/Capture/Deploy request is not proof that the crate was collected,
//! the building entered, or the yard deployed. A task remains reserved until
//! the *observable* world confirms its outcome or progress genuinely stops.
//! Only legitimate current observations are used; invisible enemy state is
//! never treated as an execution receipt.

use std::collections::BTreeMap;

use crate::rules::ruleset::RuleSet;
use crate::sim::control::Goal;
use crate::sim::intern::InternedId;
use crate::sim::control::observation::Observation;

use super::census::Census;
use super::tracked::TrackedKind;
use super::squared_distance;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum JobKind {
    Crate,
    Engineer,
    Spy,
    Garrison,
    Yard,
    Recycle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    Traveling,
    AwaitingResult,
    Interrupted,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Job {
    pub kind: JobKind,
    pub target: Option<u64>,
    pub cell: Option<(u16, u16)>,
    pub phase: Phase,
    pub assigned_at: u64,
    pub last_progress: u64,
    pub last_cell: (u16, u16),
    pub last_requested: u64,
    pub refusals: u8,
    pub expected_building: Option<InternedId>,
}

impl Job {
    fn same(&self, kind: JobKind, target: Option<u64>, cell: Option<(u16, u16)>) -> bool {
        self.kind == kind && self.target == target && self.cell == cell
    }

    fn stale(&self, now: u64) -> bool {
        // A long-distance mission must be allowed to travel for as long as it
        // makes progress, not merely for one tracked Move's 450-frame term.
        let no_progress = now.saturating_sub(self.last_progress);
        match self.kind {
            JobKind::Crate => no_progress >= 240 || now.saturating_sub(self.assigned_at) >= 600,
            JobKind::Garrison => no_progress >= 480,
            JobKind::Engineer | JobKind::Spy => no_progress >= 450
                || now.saturating_sub(self.assigned_at)>=3600,
            JobKind::Yard => no_progress >= 720,
            JobKind::Recycle => no_progress >= 600,
        }
    }
}

#[derive(Default, Debug)]
pub(super) struct TaskLedger {
    jobs: BTreeMap<u64, Job>,
}

impl TaskLedger {
    pub fn new() -> Self { Self::default() }

    pub fn get(&self, actor: u64) -> Option<Job> { self.jobs.get(&actor).copied() }

    pub fn entries(&self) -> impl Iterator<Item = (u64, Job)> + '_ {
        self.jobs.iter().map(|(actor, job)| (*actor, *job))
    }

    pub fn reserved(&self, kind: JobKind, target: u64, except: u64) -> bool {
        self.jobs.iter().any(|(actor, job)| *actor != except
            && job.kind == kind && job.target == Some(target))
    }

    pub fn pending_at(&self, kind: JobKind, target: u64) -> usize {
        self.jobs.values().filter(|job|
            job.kind == kind && job.target == Some(target)).count()
    }

    pub fn active(&self, actor: u64, kind: JobKind) -> bool {
        self.jobs.get(&actor).is_some_and(|job| job.kind == kind)
    }

    pub fn release(&mut self, actor: u64) {
        self.jobs.remove(&actor);
    }

    /// Call only after the frame outbox has accepted the proposed order.
    pub fn claim(&mut self, now: u64, actor: u64, kind: JobKind,
        target: Option<u64>, cell: Option<(u16, u16)>, from: (u16, u16)) {
        if self.jobs.get(&actor).is_some_and(|job| job.same(kind, target, cell)) {
            if let Some(job) = self.jobs.get_mut(&actor) { job.last_requested = now; }
            return;
        }
        self.jobs.insert(actor, Job {
            kind, target, cell, phase: Phase::Traveling,
            assigned_at: now, last_progress: now, last_cell: from,
            last_requested: now, refusals: 0, expected_building: None,
        });
    }

    /// Track which actual building type the mobile yard is expected to deploy.
    /// A missing MCV without that building is not proof of a successful deploy.
    pub fn yard_type(&mut self, actor: u64, ty: InternedId) {
        if let Some(job) = self.jobs.get_mut(&actor)
            && job.kind == JobKind::Yard
        { job.expected_building = Some(ty); }
    }

    pub fn yard_requested(&mut self, now: u64, actor: u64) {
        if let Some(job) = self.jobs.get_mut(&actor)
            && job.kind == JobKind::Yard
        { job.last_requested = now; job.phase = Phase::AwaitingResult; }
    }

    pub fn yard_retry_due(&self, now: u64, actor: u64) -> bool {
        self.jobs.get(&actor).is_some_and(|job|
            job.kind == JobKind::Yard && job.phase == Phase::AwaitingResult
                && now.saturating_sub(job.last_requested) >= 150)
    }

    /// MCV placement is a multi-command mission: walking to a legal site and
    /// pressing Deploy are steps of one task, not competing assignments.
    pub fn yard_site(&mut self, now: u64, actor: u64,
        cell: (u16, u16), from: (u16, u16)) {
        if let Some(job) = self.jobs.get_mut(&actor)
            && job.kind == JobKind::Yard
        {
            if job.cell != Some(cell) {
                job.cell = Some(cell);
                job.last_progress = now;
            }
        } else {
            self.claim(now, actor, JobKind::Yard, None, Some(cell), from);
        }
    }

    /// All passes, including generic `issue`, use this gate. A tactical dodge
    /// is a temporary interruption, not permission to erase the assigned job.
    pub fn permits(&self, actor: u64, kind: Option<TrackedKind>, goal: Goal) -> bool {
        let Some(job) = self.jobs.get(&actor) else { return true };
        if matches!(kind, Some(TrackedKind::Dodge | TrackedKind::Avoid)) {
            return true;
        }
        match job.kind {
            JobKind::Crate => matches!(goal, Goal::Move { cell } if Some(cell) == job.cell),
            JobKind::Engineer | JobKind::Spy | JobKind::Garrison =>
                matches!(goal, Goal::Capture { target } if Some(target) == job.target),
            JobKind::Yard => matches!(goal, Goal::Move { .. } | Goal::Deploy),
            JobKind::Recycle => matches!(goal, Goal::Grind { target }
                if Some(target) == job.target),
        }
    }

    /// A native L2 refusal must never become a phantom running task. After a
    /// small number of genuine refusals release the actor for fresh planning.
    pub fn refused(&mut self, actor: u64, goal: Goal) {
        let Some(job) = self.jobs.get_mut(&actor) else { return };
        if !Self::job_goal(job, goal) { return; }
        job.refusals = job.refusals.saturating_add(1);
        job.phase = Phase::Interrupted;
        if job.refusals >= 3 { self.release(actor); }
    }

    fn job_goal(job: &Job, goal: Goal) -> bool {
        match job.kind {
            JobKind::Crate => matches!(goal, Goal::Move { cell } if Some(cell) == job.cell),
            JobKind::Engineer | JobKind::Spy | JobKind::Garrison =>
                matches!(goal, Goal::Capture { target } if Some(target) == job.target),
            JobKind::Yard => matches!(goal, Goal::Move { .. } | Goal::Deploy),
            JobKind::Recycle => matches!(goal, Goal::Grind { target }
                if Some(target) == job.target),
        }
    }

    /// A job's own pass may repeat a lost command after tactical interference.
    /// Never repeat while the native destination is still the promised one.
    pub fn needs_retry(&self, now: u64, actor: u64,
        destination: Option<(u16, u16)>) -> bool {
        let Some(job) = self.jobs.get(&actor) else { return false };
        now.saturating_sub(job.last_requested) >= 75
            && destination != job.cell
            && now.saturating_sub(job.last_progress) < 240
    }

    pub fn reconcile(&mut self, now: u64, rules: &RuleSet,
        obs: &Observation<'_>, census: &Census) -> Vec<(u64,Job)> {
        let mut ended = Vec::new();
        let mut abandoned=Vec::new();
        for (&actor, job) in &mut self.jobs {
            if job.kind==JobKind::Garrison && obs.owns_object(actor)
                && job.target.is_some_and(|target|obs.inside_transport(actor)==Some(target)
                    && obs.owns_object(target)
                    && obs.entity_type(rules,target).is_some_and(|ty|
                        ty.category==crate::rules::object_type::ObjectCategory::Building))
            {
                if super::trace_enabled("AUTOPLAY_V56_TRACE") {
                    log::info!("auto play: v56_garrison_confirmed tick={now} actor={actor} target={:?}",job.target);
                }
                ended.push(actor);
                continue;
            }
            let Some(unit) = census.units.iter().find(|unit| unit.entity_id == actor) else {
                // Do not call an MCV successful merely because it vanished:
                // verify an owned yard of the *declared* type near the site.
                if job.kind == JobKind::Yard {
                    let confirmed = job.expected_building.is_some_and(|expected|
                        obs.own_entities().any(|entity| entity.type_id == expected
                            && job.cell.is_some_and(|site|
                                squared_distance(site, entity.cell) <= 4 * 4)));
                    log::info!("auto play: MCV {actor} ended: yard deployed confirmed={confirmed}");
                }
                // Spy disappearance is likewise not a success receipt.
                ended.push(actor);
                continue;
            };
            if unit.hand || !unit.orderable {
                if job.kind != JobKind::Garrison
                    || obs.inside_transport(actor) != job.target
                {
                    ended.push(actor);
                } else {
                    ended.push(actor); // verified occupancy
                }
                continue;
            }
            if job.kind == JobKind::Garrison
                && obs.inside_transport(actor) == job.target
            {
                ended.push(actor);
                continue;
            }
            if job.kind == JobKind::Recycle {
                // A mind-control release or lost grinder ends the trip;
                // disappearance alone is not a certified refund receipt.
                let grinder_owned = job.target.is_some_and(|id| obs.own_entities().any(|site|
                    site.entity_id == id && site.orderable
                    && obs.entity_type(rules,id).is_some_and(|ty| ty.grinding)));
                if !obs.controlled_by_own_controller(actor) || !grinder_owned {
                    ended.push(actor);
                    continue;
                }
            }
            if job.kind == JobKind::Engineer
                && job.target.is_some_and(|id|
                    obs.own_entities().any(|own|own.entity_id==id)
                    && !obs.can_take_building(rules,actor,id))
            {
                ended.push(actor);
                continue;
            }
            if job.kind == JobKind::Crate
                && job.cell.is_some_and(|cell| obs.is_cell_visible(cell.0, cell.1)
                    && !obs.visible_crate_at(rules, cell))
            {
                // Even if another unit took the crate, the claim is over.
                ended.push(actor);
                continue;
            }
            if unit.cell != job.last_cell {
                job.last_cell = unit.cell;
                job.last_progress = now;
                job.phase = Phase::Traveling;
            } else if job.cell.is_some_and(|cell| squared_distance(cell, unit.cell) <= 2)
                || job.target.is_some_and(|target| obs.attack_target(actor) == Some(target))
            {
                job.phase = Phase::AwaitingResult;
            }
            if job.stale(now) {
                log::debug!("auto play: {:?} job for {actor} made no progress; release", job.kind);
                if matches!(job.kind,JobKind::Engineer|JobKind::Spy|JobKind::Garrison) {
                    abandoned.push((actor,*job));
                }
                ended.push(actor);
            }
        }
        for actor in ended { self.jobs.remove(&actor); }
        abandoned
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;

    #[test]
    fn v61_a_specialist_cannot_renew_a_wandering_job_forever() {
        let mut tasks=TaskLedger::new();
        tasks.claim(10,8,JobKind::Engineer,Some(20),Some((40,40)),(2,2));
        let job=tasks.jobs.get_mut(&8).unwrap();
        job.last_progress=3609;
        assert!(!job.stale(3609));
        assert!(job.stale(3610));
    }

    #[test]
    fn crate_goal_is_exclusive_except_for_a_short_emergency_step() {
        let mut tasks = TaskLedger::new();
        tasks.claim(20, 8, JobKind::Crate, None, Some((40, 40)), (8, 8));
        assert!(tasks.permits(8, Some(TrackedKind::Move), Goal::Move { cell: (40, 40) }));
        assert!(!tasks.permits(8, Some(TrackedKind::AttackMove),
            Goal::AttackMove { cell: (45, 45) }));
        assert!(!tasks.permits(8, Some(TrackedKind::Move), Goal::Move { cell: (5, 5) }));
        assert!(tasks.permits(8, Some(TrackedKind::Dodge), Goal::Move { cell: (9, 9) }));
        tasks.release(8);
        assert!(tasks.permits(8, Some(TrackedKind::AttackMove),
            Goal::AttackMove { cell: (45, 45) }));
    }

    #[test]
    fn grinder_trip_reserves_the_controlled_unit_but_allows_emergency_dodge() {
        let mut tasks = TaskLedger::new();
        tasks.claim(100, 12, JobKind::Recycle, Some(700), Some((18, 18)), (4, 4));
        assert!(tasks.permits(12, Some(TrackedKind::Enter), Goal::Grind { target: 700 }));
        assert!(!tasks.permits(12, Some(TrackedKind::AttackMove),
            Goal::AttackMove { cell: (25, 25) }));
        assert!(!tasks.permits(12, Some(TrackedKind::Enter), Goal::Grind { target: 701 }));
        assert!(!tasks.permits(12, Some(TrackedKind::Enter), Goal::Enter { target: 700 }));
        assert!(tasks.permits(12, Some(TrackedKind::Dodge),
            Goal::Move { cell: (5, 5) }));
        tasks.release(12);
        assert!(tasks.permits(12, Some(TrackedKind::Guard), Goal::Guard));
    }

    #[test]
    fn specialist_claim_survives_repeat_order_and_yard_has_multiple_steps() {
        let mut tasks = TaskLedger::new();
        tasks.claim(0, 12, JobKind::Engineer, Some(500), Some((40, 40)), (0, 0));
        tasks.claim(75, 12, JobKind::Engineer, Some(500), Some((40, 40)), (0, 0));
        assert_eq!(tasks.get(12).expect("engineer job").assigned_at, 0);
        assert!(!tasks.permits(12, Some(TrackedKind::Capture), Goal::Capture { target: 501 }));
        assert!(tasks.permits(12, Some(TrackedKind::Capture), Goal::Capture { target: 500 }));
        tasks.yard_site(100, 17, (30, 30), (5, 5));
        assert!(tasks.permits(17, None, Goal::Move { cell: (30, 30) }));
        assert!(tasks.permits(17, None, Goal::Deploy));
        assert!(!tasks.permits(17, None, Goal::AttackMove { cell: (30, 30) }));
    }

    #[test]
    fn rejection_releases_only_after_bounded_real_failures() {
        let mut tasks = TaskLedger::new();
        tasks.claim(0, 9, JobKind::Garrison, Some(80), Some((10, 10)), (2, 2));
        for _ in 0..2 { tasks.refused(9, Goal::Capture { target: 80 }); }
        assert!(tasks.active(9, JobKind::Garrison));
        tasks.refused(9, Goal::Capture { target: 80 });
        assert!(!tasks.active(9, JobKind::Garrison));
    }
}
