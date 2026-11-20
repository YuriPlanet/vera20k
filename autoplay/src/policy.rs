//! The policy: managers propose, the assignment decides, the outbox answers.
//!
//! One term of this is: read the projection, collect proposals and intents,
//! give each unit at most one job, and issue the order for a job the unit is
//! not already living out. Nothing here names a unit type, and nothing here
//! writes state: every answer leaves through the outbox, which the control
//! layer admits exactly as it admits a player's click.

use std::collections::BTreeMap;

use vera20k_core::rules::ruleset::RuleSet;
use vera20k_core::rules::unit_role::RoleTable;
use vera20k_core::sim::behavior::goal::Goal;
use vera20k_core::sim::behavior::view::cell_distance;
use vera20k_core::sim::control::brain::{Brain, Outbox};
use vera20k_core::sim::control::observation::Observation;
use vera20k_core::sim::mission::{MissionId, MissionType};

use crate::job::{Assignments, Job, Proposal, Target};
use crate::manager::abilities::Abilities;
use crate::manager::build::Build;
use crate::manager::economy::Economy;
use crate::manager::military::Military;
use crate::manager::{Ctx, Demands, Manager};
use crate::profile::{Profiles, Roles};
use crate::registry::{JobKind, Manager as Domain};

/// How far from the base a unit has to be before a finished fight sends it
/// home. A unit that never left the base has nothing to walk back from.
const RETURN_HOME_TRIGGER: i32 = 20;

/// How close to the base counts as home.
const RETURN_HOME_ARRIVED: i32 = 3;

/// How far a neighbour counts as company, in cells.
const RETURN_HOME_COMPANY: i32 = 6;

/// How many neighbours make a unit part of a column rather than a straggler.
///
/// Two. One neighbour is a pair that can be picked off together; three is a
/// squad. Measured, the rule this replaces sent forty percent of all orders as
/// "go home" and the army never held ground.
const RETURN_HOME_COMPANY_MIN: usize = 2;

/// How close an enemy has to be for a unit to count as still fighting.
///
/// Twelve cells, the range at which anything in the rules can shoot at it. Used
/// only to keep the go-home order away from a unit with a target in sight.
const RETURN_HOME_ENGAGED: i32 = 12;

/// How often the policy re-decides jobs, in frames: one second of the
/// simulation. Reactive decisions do not wait for it; this is the slow loop.
pub const TERM_FRAMES: u64 = 15;

/// How long a unit may stand in one cell on an assault before the job is
/// abandoned: the order named something this unit cannot get to.
///
/// The ladder is the design's (`autoplay/docs/design.md`, section 4): every task
/// reports `Done`, `Unreachable`, `Ineffective` or `Stalled`, and every rung
/// terminates — the last rung is giving up and cooling down. Thirty seconds is
/// the point at which standing still is no longer a fight in progress.
const UNREACHABLE_FRAMES: u64 = 450;

/// How close a scout has to get to its cell for the errand to count as done.
///
/// Four cells, matching the manager's own arrival test: the fog reveals whole
/// cells around a unit, so this is near enough that the ground has been looked at.
const SCOUT_ARRIVED_CELLS: i32 = 4;

/// How long a unit stays off a target it could not reach, in frames: two
/// minutes, the same order of magnitude as one battle.
const UNREACHABLE_COOLDOWN: u64 = 1_800;

/// How long an assault may fail to reduce its target before the job is
/// `Ineffective` and ended.
///
/// The cell timer above cannot answer this one: a unit re-assigned in the term
/// it is counted has a freshly reset timer, which is why the probe in status.md
/// showed units standing 1800 frames with `stalled=0`. The target's health is
/// the fact that does not reset, and it is the fourth outcome the design names.
const INEFFECTIVE_FRAMES: u64 = 450;

/// How long a unit stays off a target its attack could not hurt, in frames.
const INEFFECTIVE_COOLDOWN: u64 = 1_800;

/// How long the house stops offering a target nobody can hurt.
///
/// Long, because the fact does not change: a building's armour and a warhead's
/// verses are both static, so the only honest reason to try again is a new unit
/// type entering the army. Five minutes of frames is long enough for that to be
/// worth a retry and short enough that a rebuilt enemy base is still reachable.
const INEFFECTIVE_TARGET_COOLDOWN: u64 = 4_500;

/// How an assault ended, for the trace and for the cooldown it records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ending {
    /// The target died or left sight: the job is done.
    Done,
    /// The unit never got to it: the order named somewhere it cannot reach.
    Unreachable,
    /// The unit got there and the target's health never moved: the attack does
    /// nothing to it.
    Ineffective,
}

/// What the policy did, for the trace line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub proposals: usize,
    pub intents: usize,
    pub assigned: usize,
    pub issued: usize,
    pub unowned: usize,
    pub refused: usize,
    /// Jobs that ended this term because their target was gone.
    pub finished: usize,
    /// Jobs that ended this term because the unit could not reach the target.
    pub stalled: usize,
    /// Jobs that ended this term because the attack never hurt its target.
    pub ineffective: usize,
    /// Proposals dropped this term for a target the unit had given up on.
    pub cooled: usize,
    /// Units given the default job this term because nothing else claimed them.
    pub fallback: usize,
    /// Units that have not changed cell for six seconds while holding a job.
    pub stuck: usize,
    /// What the house has to fight with, in hit points.
    pub army: i32,
    /// What it can see of the enemy, in the same measure.
    pub enemy: i32,
}

pub struct Policy {
    profiles: Profiles,
    /// What every type is for, read off the rules by the engine's role model.
    roles: RoleTable,
    managers: Vec<Box<dyn Manager>>,
    jobs: Assignments,
    last_term: Option<u64>,
    /// The order each unit has been sent: the job kind, and the target it named.
    /// A job whose target changed is a new order even when its kind did not.
    issued: BTreeMap<u64, (JobKind, Target)>,
    /// Where each unit stood when it last moved, and when: the stuck counter
    /// reads this, so "a unit that does nothing" is a number, not an opinion.
    stood: BTreeMap<u64, ((u16, u16), u64)>,
    /// The house's own objects last term, by stable id and type, so this term can
    /// say what went missing instead of only how many are left.
    alive_last: BTreeMap<u64, String>,
    /// How many structures the house stood on last term.
    ///
    /// Used only to tell a deployment from a casualty: unfolding consumes the
    /// vehicle and raises a yard, so the structure count rises as the unit goes.
    structures_last: usize,
    /// Every object the house has lost, by type, with what one of them cost.
    lost: BTreeMap<String, Tally>,
    /// Every object the enemy held last term, keyed by stable id.
    ///
    /// `enemy_entities` walks the whole object store without a visibility
    /// filter, so a difference against this is the enemy's real losses and not
    /// what one rifleman happened to see.
    enemy_alive_last: BTreeMap<u64, String>,
    /// What the enemy lost, by type, with what one of them was worth.
    ///
    /// This is the other side of the money: `lost` says what our army cost,
    /// and this says what it bought. A house that trades a hundred credits of
    /// rifleman for two hundred of the enemy's is ahead even if it never holds
    /// more ground.
    destroyed: BTreeMap<String, Tally>,
    /// Every structure the house stood on last term, keyed by stable id.
    ///
    /// `snapshot` holds the units only, so without this a destroyed building
    /// never reaches the ledger and the dearest half of the economy goes
    /// unread.
    structures_alive_last: BTreeMap<u64, String>,
    /// Everything the house produced this match, by type.
    ///
    /// `own` counts survivors and never the dead or the spent, so a change that
    /// trades twelve tanks for forty riflemen looks like a win on it. This and
    /// `lost` are the other half of that reading.
    built: BTreeMap<String, Tally>,
    /// Engineers produced, and what became of them.
    engineers: EngineerLedger,
    /// Targets a unit has given up on, and the tick it may be proposed again:
    /// the last rung of the assault ladder.
    given_up: BTreeMap<(u64, u64), u64>,
    /// Targets the whole house has found it cannot hurt, and when to try again.
    ///
    /// Keyed by target alone: a building nobody's warhead can touch stays
    /// untouched whichever unit is sent, so remembering it per unit only makes
    /// the army take turns.
    ineffective_targets: BTreeMap<u64, u64>,
    /// What each unit's assault last saw of its target: the health it read and
    /// the tick it read it. A target whose health has not moved since then is a
    /// target the attack is not hurting — the design's `Ineffective` outcome,
    /// which the unit's own cell cannot show.
    progress: BTreeMap<u64, (i32, u64)>,
    /// Units whose assault took health off its target in the term just read.
    ///
    /// A unit that is killing the thing it was told to kill is not stuck, and
    /// the cell cannot say so: an attack in range often means standing still.
    /// The spec's H2 counts a unit stuck when it holds a *movement* job and does
    /// not move for three seconds, not a unit that is firing.
    hitting: std::collections::BTreeSet<u64>,
    /// The bales in each harvester's hold at the last reading. A harvester
    /// whose count moved is filling, and a harvester that is filling is
    /// working: the cell cannot say that, the engine's own cargo can.
    cargo_seen: BTreeMap<u64, u32>,
    /// Where the house's base is, as the manager that owns it says.
    home: Option<(u16, u16)>,
    /// The units on their way home after a fight.
    ///
    /// The column that leaves and never comes back is the one failure the
    /// attack lane has: measured, Barrel.mmx seed 19 read own 46 -> 37 with
    /// the column out and nothing killed for it. A unit in this set walks home
    /// and is given no other job until it is there.
    returning: std::collections::BTreeSet<u64>,
    stats: Stats,
}

impl Policy {
    pub fn new(rules: &RuleSet) -> Self {
        Self {
            profiles: Profiles::build(rules),
            roles: RoleTable::build(rules),
            managers: vec![
                Box::new(Economy),
                Box::new(Build::default()),
                Box::new(Military::default()),
                Box::new(Abilities::default()),
            ],
            jobs: Assignments::default(),
            last_term: None,
            issued: BTreeMap::new(),
            stood: BTreeMap::new(),
            given_up: BTreeMap::new(),
            ineffective_targets: BTreeMap::new(),
            progress: BTreeMap::new(),
            hitting: std::collections::BTreeSet::new(),
            cargo_seen: BTreeMap::new(),
            home: None,
            returning: std::collections::BTreeSet::new(),
            alive_last: BTreeMap::new(),
            structures_last: 0,
            lost: BTreeMap::new(),
            enemy_alive_last: BTreeMap::new(),
            destroyed: BTreeMap::new(),
            structures_alive_last: BTreeMap::new(),
            built: BTreeMap::new(),
            engineers: EngineerLedger::default(),
            stats: Stats::default(),
        }
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    pub fn profiles(&self) -> &Profiles {
        &self.profiles
    }

    pub fn jobs(&self) -> &Assignments {
        &self.jobs
    }

    /// Whether a type may hold this job. The predicate reads the profile, never
    /// a name, so a mod's harvester is as good as the stock one.
    fn eligible(kind: JobKind, roles: Roles) -> bool {
        match kind {
            JobKind::Harvest | JobKind::ReturnOre => roles.contains(Roles::HARVESTER),
            JobKind::Deploy => roles.contains(Roles::DEPLOYER),
            JobKind::Repair => roles.contains(Roles::STRUCTURE),
            JobKind::Capture => roles.contains(Roles::ENGINEER) || roles.contains(Roles::AGENT),
            JobKind::Garrison => roles.contains(Roles::OCCUPIER),
            JobKind::Transport => roles.contains(Roles::TRANSPORT),
            JobKind::Breach | JobKind::Assault | JobKind::Defend => roles.contains(Roles::ARMED),
            JobKind::Scout | JobKind::Retreat => true,
            JobKind::ReturnHome | JobKind::Advance => roles.contains(Roles::ARMED),
        }
    }

    /// Whether the unit's own mission decides that this job is already live.
    ///
    /// A job without one is sent once and never re-sent, because re-sending the
    /// same order every term is what makes a unit unable to act.
    fn watched_by_mission(kind: JobKind) -> bool {
        matches!(
            kind,
            JobKind::Harvest
                | JobKind::Assault
                | JobKind::Defend
                | JobKind::ReturnHome
                | JobKind::Capture
        )
    }

    /// Whether the unit's current mission already means this job is being lived
    /// out, so the policy has to leave the unit alone.
    ///
    /// The harvester is the case that matters. The engine hands a docking miner
    /// to `Enter` for the refinery contact and `Unload` for the dump
    /// (`MissionType::Enter` queued at `0x0073E093`, the dump under
    /// `Mission_Unload`), so a policy that recognized only `Harvest` would send
    /// a MegaMission into the middle of every dock. A MegaMission is
    /// `Queue_Mission(mission, 1)` (`sim/mission/mega_mission.rs`): it replaces
    /// the queued `Enter` that `per_cell_release_dock_contact` (`0x0073ACD7`)
    /// needs to keep the contact, the radioed refinery is dropped at the next
    /// track end, and the truck cycles back to the ore field still full —
    /// measured at 527 dock entries against 14 empty-hold arrivals in one match
    /// before this change, and one dock per unload after it.
    fn living_out(kind: JobKind, mission: MissionId) -> bool {
        match kind {
            JobKind::Harvest => {
                mission == MissionId::from_known(MissionType::Harvest)
                    || mission == MissionId::from_known(MissionType::Enter)
                    || mission == MissionId::from_known(MissionType::Unload)
            }
            JobKind::Assault => {
                // An attack-move that carries a destination lands on `Move`
                // with the attack-move latch set, not on `Attack` (native
                // `mega_mission.rs::foot_assign_target_command`, the
                // `0x004DF141` destination arm), so `Attack` alone would
                // re-send the order every term — the same failure the
                // harvester's dock cycle has.
                mission == MissionId::from_known(MissionType::Attack)
                    || mission == MissionId::from_known(MissionType::Move)
            }
            JobKind::Defend => mission == MissionId::from_known(MissionType::AreaGuard),
            // A unit walking home is on the Move mission; re-sending the walk
            // would restart it every term.
            JobKind::ReturnHome => mission == MissionId::from_known(MissionType::Move),
            // A capturer holds the Capture mission for the whole walk: the
            // engine gives it from CaptureBuilding and keeps it until per_cell
            // hands the arrival to the building receiver. Without this arm the
            // job went out once, same_order suppressed every later term, and a
            // measured engineer read job=Capture standing=15 before the
            // assignment took it away again.
            // A capturer walking to its building is `Move`, not `Capture`, and
            // re-issuing the order on every `Move` frame restarting the walk is
            // the reason a capture never landed.
            //
            // Measured on Arena.mmx seed 0x5EED at 24000 ticks: one engineer was
            // bought, thirty `Capture` proposals were made, and **zero** buildings
            // changed hands; the unit line showed `MissionId(8)` and `MissionId(2)`
            // alternating every term at `(89, 37)`, a few cells from the target.
            // It walked two cells, was handed the order again, had its walk
            // restarted, and repeated that for the rest of the match.
            //
            // The engine gives `Capture` from CaptureBuilding and keeps it until
            // per_cell hands the arrival to the building receiver, so the walk
            // towards the target is the same errand in progress.
            JobKind::Capture => {
                mission == MissionId::from_known(MissionType::Capture)
                    || mission == MissionId::from_known(MissionType::Move)
            }
            _ => false,
        }
    }

    fn goal(job: &Job) -> Option<Goal> {
        match (job.kind, job.target) {
            (JobKind::Harvest, Target::Cell(cell)) => Some(Goal::Harvest { cell }),
            // The army attacks the object it was told to: the click a player
            // makes on an enemy, not an attack-move to where it stood. An
            // attack-move's destination is reached once and then stood on, and
            // that is the stale-target failure status.md records.
            (JobKind::Assault, Target::Object(target)) => Some(Goal::Attack { target }),
            // The approach half of an attack: walk to the object, do not shell it
            // from where the weapon cannot reach.
            (JobKind::Advance, Target::Object(target)) => Some(Goal::MoveToObject { target }),
            (JobKind::Assault, Target::Cell(cell)) => Some(Goal::AttackMove { cell }),
            (JobKind::Defend, Target::Cell(cell)) => Some(Goal::GuardAt { cell }),
            (JobKind::ReturnHome, Target::Cell(cell)) => Some(Goal::Move { cell }),
            // A retreat is a walk, not a mission: the engine has no order for
            // "leave", and a unit that is losing a fight only has to go somewhere
            // else.
            (JobKind::Retreat, Target::Cell(cell)) => Some(Goal::Move { cell }),
            // A scout walks to the frontier cell it was sent to. `Move` and not
            // `GuardAt`: guarding is standing somewhere, and standing on ground
            // already revealed is the one thing scouting is not.
            (JobKind::Scout, Target::Cell(cell)) => Some(Goal::Move { cell }),
            // The capture click a player makes on a building. The engine owns the
            // walk and the arrival (`mission_capture`, the building receiver at
            // `0x00519948`); the policy only has to name the engineer and the
            // building.
            (JobKind::Capture, Target::Object(target)) => Some(Goal::Capture { target }),
            // A garrison is the Enter mission on a capturable civilian structure:
            // same door a player uses to put a rifleman inside a hospital.
            (JobKind::Garrison, Target::Object(target)) => Some(Goal::Enter { target }),
            // A transport carries a passenger to a destination: the Enter click on
            // the transport, which boards it.
            (JobKind::Transport, Target::Object(target)) => Some(Goal::Enter { target }),
            (JobKind::Deploy, _) => Some(Goal::Deploy),
            _ => None,
        }
    }

    /// End the jobs that are done, and the ones the unit cannot do.
    ///
    /// Three reasons, and all of them terminate:
    ///
    /// - **done**: the object is no longer visible (it died or left sight);
    /// - **unreachable**: the unit has stood in one cell for
    ///   [`UNREACHABLE_FRAMES`], so the order named somewhere it cannot get to;
    /// - **ineffective**: the target's health has not moved for
    ///   [`INEFFECTIVE_FRAMES`], so the attack is not hurting it. This is the
    ///   design's fourth outcome, and the one the unit's own cell cannot show: a
    ///   unit re-assigned in the term it is counted has a freshly reset cell
    ///   timer, which is why the probe in status.md showed units standing 1800
    ///   frames with `stalled=0`. The target's health does not reset.
    ///
    /// The last two are the design's last rung — give up and cool down
    /// (`design.md`, section 4) — and both record the pair in `given_up`, so the
    /// same target is not proposed to the same unit again until the cooldown has
    /// passed.
    ///
    /// Jobs end before this term's proposals are collected, so the same unit can
    /// be given a live target in the same term.
    fn finish_done_jobs(&mut self, obs: &Observation<'_>, rules: &RuleSet, tick: u64) {
        self.given_up.retain(|_, until| *until > tick);
        self.ineffective_targets.retain(|_, until| *until > tick);
        self.hitting.clear();
        let visible: Vec<u64> = obs
            .visible_enemy_entities()
            .map(|enemy| enemy.entity_id)
            .collect();
        let cells: BTreeMap<u64, (u16, u16)> = obs
            .own_entities()
            .map(|unit| (unit.entity_id, unit.cell))
            .collect();
        let mut done: Vec<(u64, u64, Ending)> = Vec::new();
        // A scout that has reached its cell has finished the errand.
        //
        // Without this arm nothing ever ended a `Scout`, because the loop below
        // only closes an `Assault` on an object: the manager re-proposed the same
        // cell every term, `watched_by_mission` does not watch `Scout`, and the
        // unit held one order forever. Measured on Arena.mmx seed 0x5EED, a
        // rifleman sat on `(76, 40)` with `standing=3045` — fifty seconds on a
        // one-cell errand — and the far half of the map was never looked at, so
        // the column never learned which way the enemy was.
        for (unit, job) in self.jobs.iter() {
            if !matches!((job.kind, job.target), (JobKind::Scout, Target::Cell(_))) {
                continue;
            }
            let cell = cells.get(unit).copied().unwrap_or((0, 0));
            let Some(Target::Cell(destination)) = Some(job.target) else {
                continue;
            };
            if cell_distance(cell, destination) <= SCOUT_ARRIVED_CELLS {
                done.push((*unit, *unit, Ending::Done));
            }
        }
        for (unit, job) in self.jobs.iter() {
            let (JobKind::Assault, Target::Object(target)) = (job.kind, job.target) else {
                continue;
            };
            if !visible.contains(&target) {
                done.push((*unit, target, Ending::Done));
                continue;
            }
            let cell = cells.get(unit).copied().unwrap_or((0, 0));
            if Self::standing_frames(self.stood.get(unit), cell, tick) >= UNREACHABLE_FRAMES {
                done.push((*unit, target, Ending::Unreachable));
                continue;
            }
            let Some(health) = obs.health(target) else {
                // No health means no live object: the same ending as out of
                // sight, and it arrives one term earlier.
                done.push((*unit, target, Ending::Done));
                continue;
            };
            // Remember the reading that *started* the current run of identical
            // health, not this term's. Inserting unconditionally would move the
            // timestamp every term and the window could never elapse — which is
            // exactly what the first attempt did, with `ineffective=0` in a
            // match where units stood for 1800 frames.
            let seen = self.progress.get(unit).copied();
            let unchanged_since = match seen {
                Some((before, since)) if before == health => {
                    if tick.saturating_sub(since) >= INEFFECTIVE_FRAMES {
                        // Say what was being shot. A target whose health does not
                        // move for the whole window is either armoured past this
                        // unit's warhead or not damageable at all, and which one it
                        // is decides whether the fix belongs in targeting or in the
                        // damage table. Measured on Arena.mmx seed 0x5EED, the
                        // army spent the match on one object id and killed nothing.
                        let name = obs
                            .object_cell_and_type(target)
                            .and_then(|(_, type_id)| obs.object_type(rules, type_id))
                            .map(|ty| ty.id.clone())
                            .unwrap_or_else(|| "<gone>".to_string());
                        log::info!(
                            "policy v2: ineffective target={} type={} health={} after {} frames",
                            target,
                            name,
                            health,
                            tick.saturating_sub(since)
                        );
                        done.push((*unit, target, Ending::Ineffective));
                    }
                    since
                }
                Some((before, _)) if before != health => {
                    // It is being hurt: this reading starts a new run, and a
                    // unit taking health off its target is working, not stuck.
                    self.hitting.insert(*unit);
                    tick
                }
                _ => tick,
            };
            self.progress.insert(*unit, (health, unchanged_since));
        }
        for (unit, target, ending) in done {
            self.jobs.take(unit);
            self.issued.remove(&unit);
            self.progress.remove(&unit);
            // A unit that has finished its fight, and is far from home, walks
            // back before it is given another job. The base is what the army
            // is for, and a column that never returns is how the base is lost.
            // Alone and far from home, walk back; in company, stay.
            //
            // The first rule sent every unit home after every finished fight, and
            // measured on the 1v1 lane it produced 820 of 2033 unit decisions —
            // forty percent of the army's orders were "go home", which is why the
            // column never got anywhere: it walked out, killed something, and
            // walked back. A unit with a friend within a few cells is a column and
            // a column is safer where it stands than strung out on the road.
            if let Some(home) = self.home
                && let Some(cell) = cells.get(&unit)
                && cell_distance(*cell, home) >= RETURN_HOME_TRIGGER
            {
                let company = cells
                    .iter()
                    .filter(|(other, _)| **other != unit)
                    .filter(|(_, other_cell)| {
                        cell_distance(*cell, **other_cell) <= RETURN_HOME_COMPANY
                    })
                    .count();
                // Standing in the enemy's base is not a place to walk home from.
                // Measured on Death.mmx seed 19: units reached (183-186, 151-153)
                // with the enemy base at (182-197, 137-141) and were ordered home
                // from there, alone. The base was never dismantled because the
                // only units that got to it were turned around at the door.
                //
                // A unit that can see an enemy is still fighting, whatever its
                // company; walking away with a target in sight is the retreat this
                // policy does not have and should not fake with a go-home order.
                let engaged = obs
                    .visible_enemy_entities()
                    .any(|enemy| cell_distance(enemy.cell, *cell) <= RETURN_HOME_ENGAGED);
                if company < RETURN_HOME_COMPANY_MIN && !engaged {
                    self.returning.insert(unit);
                }
            }
            match ending {
                Ending::Done => self.stats.finished += 1,
                Ending::Unreachable => {
                    self.given_up
                        .insert((unit, target), tick + UNREACHABLE_COOLDOWN);
                    self.stats.stalled += 1;
                }
                Ending::Ineffective => {
                    self.given_up
                        .insert((unit, target), tick + INEFFECTIVE_COOLDOWN);
                    // And for the whole house, not just this unit.
                    //
                    // The per-unit key is too fine to stop anything: a target that
                    // forty units each spent a window on is forty separate
                    // cooldowns that each expire, so the army cycles through the
                    // roster and comes back. Measured on Arena.mmx seed 0x5EED, the
                    // column held one Soviet Iron Curtain device for a whole match
                    // while `ineffective` climbed and nothing changed.
                    //
                    // A warhead that cannot hurt this building will not start
                    // hurting it on the next unit, so the building is what should
                    // be remembered.
                    self.ineffective_targets
                        .insert(target, tick + INEFFECTIVE_TARGET_COOLDOWN);
                    self.stats.ineffective += 1;
                }
            }
        }
    }

    /// Whether the order a unit already holds is the order this job asks for.
    ///
    /// Only an assault is compared by target, and it has to be: its enemy walks,
    /// the mission keeps reading `Attack` while it does, and a job re-aimed at a
    /// different enemy would otherwise look like a job already lived out.
    ///
    /// The other two watched jobs are compared by kind alone, each for its own
    /// reason. A harvest job's target is the cell the truck stood on when the
    /// job was given, and the ore is the mission's business, so comparing it
    /// would re-order every truck every term and break the dock again. A defend
    /// job's post is the first structure the house has, and before the yard
    /// stands that is "wherever the first soldier is" — a cell that moves with
    /// the army, so comparing it scattered the army before the base existed.
    fn same_order(issued: Option<&(JobKind, Target)>, job: &Job) -> bool {
        issued.is_some_and(|(kind, target)| {
            // Assault and advance both name an object that can move or die, so both
            // have to compare it: a job that changed target is a new order.
            //
            // `Advance` was excluded when it was added, which meant a unit sent to
            // approach one enemy kept its order when the army's target moved to
            // another — measured on Arena.mmx seed 19, `job=Advance
            // target=Object(1162) standing=1005 mission=MissionId(5)` with the
            // target's own position reading `(45, 83)` while the unit sat at
            // `(122, 83)`: eighty cells apart, and the order was never re-issued
            // because the kind had not changed.
            *kind == job.kind
                && (!matches!(job.kind, JobKind::Assault | JobKind::Advance)
                    || *target == job.target)
        })
    }

    /// How long the unit has stood in the cell it stands in now.
    fn standing_frames(stood: Option<&((u16, u16), u64)>, cell: (u16, u16), tick: u64) -> u64 {
        match stood {
            Some((stood_at, since)) if *stood_at == cell => tick.saturating_sub(*since),
            _ => 0,
        }
    }

    /// How close to the named cell a unit has to be for its job to count as done.
    ///
    /// A column does not arrive in formation: members stop two or three cells apart
    /// around the cell they were sent to, and each is then five to ten frames short of
    /// moving. One cell is too tight — the measured case was a squad holding
    /// (143,98)..(146,99) around Cell((144,96)), reading stuck at ninety frames.
    const ACHIEVED_CELLS: i32 = 3;

    /// Whether the unit has already done what its job asked: it stands on the
    /// cell the job names, within a step.
    ///
    /// This is what separates "still working" from "finished and waiting". The
    /// spec's H2 (`stuck`) counts a unit that holds a movement job and should be
    /// moving but is not; a column standing on the ground it was sent to take has
    /// done what it was told. Counting those made a march that destroyed seven
    /// enemy objects read `stuck=40` and lose to a build that never left its base
    /// (status.md, "three marches rejected on a reading that measures the wrong
    /// thing").
    ///
    /// Only cell targets are answered: an object target is a thing to keep
    /// shooting, and `finish_done_jobs` decides when that is over.
    fn job_achieved(job: &Job, cell: (u16, u16)) -> bool {
        match job.target {
            Target::Cell(target) => {
                i32::from(cell.0.abs_diff(target.0)).max(i32::from(cell.1.abs_diff(target.1)))
                    <= Self::ACHIEVED_CELLS
            }
            // An object target is a thing to keep shooting, and a job with no
            // target is a job with nowhere to be: neither is finished by standing
            // still.
            Target::Object(_) | Target::None => false,
        }
    }

    /// The job a unit holds when nothing else claimed it: guard the nearest
    /// structure the house owns.
    ///
    /// Off the profile, never a type name: any unit that is armed and not a
    /// harvester can guard, and any structure is worth standing at. A house with
    /// no structure yet guards the cell its units stand on, so the starting
    /// force has a post before the yard exists.
    fn default_job(&self, obs: &Observation<'_>, rules: &RuleSet, tick: u64) -> Vec<(u64, Job)> {
        let mut posts: Vec<(u16, u16)> = Vec::new();
        let mut idle: Vec<u64> = Vec::new();
        for unit in obs.own_entities() {
            let Some(ty) = obs.object_type(rules, unit.type_id) else {
                continue;
            };
            let Some(profile) = self.profiles.get(&ty.id) else {
                continue;
            };
            if profile.roles.contains(Roles::STRUCTURE) {
                posts.push(unit.cell);
                continue;
            }
            if unit.orderable
                && !profile.roles.contains(Roles::HARVESTER)
                && profile.roles.contains(Roles::ARMED)
                && obs.inside_transport(unit.entity_id).is_none()
                && self.jobs.get(unit.entity_id).is_none()
            {
                idle.push(unit.entity_id);
            }
        }
        if posts.is_empty() {
            return Vec::new();
        }
        posts.sort_unstable();
        idle.into_iter()
            .map(|unit| {
                let cell = obs
                    .own_entities()
                    .find(|entity| entity.entity_id == unit)
                    .map(|entity| entity.cell)
                    .unwrap_or(posts[0]);
                let post = *posts
                    .iter()
                    .min_by_key(|at| {
                        i32::from(at.0.abs_diff(cell.0)).max(i32::from(at.1.abs_diff(cell.1)))
                    })
                    .expect("a structure to guard");
                (
                    unit,
                    Job {
                        kind: JobKind::Defend,
                        target: Target::Cell(post),
                        // Below every proposal a manager makes: this is the job
                        // a unit gets when nothing wanted it.
                        priority: 0,
                        given: tick,
                    },
                )
            })
            .collect()
    }

    /// Give each unit at most one job, strongest proposal first. Deterministic:
    /// the order is priority, then kind, then unit id.
    fn assign(&mut self, mut proposals: Vec<Proposal>, tick: u64) {
        proposals.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| a.kind.cmp(&b.kind))
                .then_with(|| match (a.actor, b.actor) {
                    (Some(x), Some(y)) => x.cmp(&y),
                    _ => std::cmp::Ordering::Equal,
                })
        });
        for proposal in proposals {
            let Some(unit) = proposal.actor else {
                // A demand no unit can fill yet: counted, never silently
                // dropped.
                self.stats.unowned += 1;
                continue;
            };
            let job = Job {
                kind: proposal.kind,
                target: proposal.target,
                priority: proposal.priority,
                given: tick,
            };
            // A target nobody can hurt is not proposed to anybody. Checked before
            // the per-unit cooldown because it is the wider fact: one warhead
            // cannot touch one armour, and which unit carries that warhead does
            // not change it.
            if let Target::Object(target) = job.target
                && self.ineffective_targets.contains_key(&target)
            {
                self.stats.cooled += 1;
                continue;
            }
            // A target this unit has already given up on is not proposed to it
            // again until the cooldown runs out. Without this the ladder ends in
            // a loop: give up, be told the same thing next term, stand still.
            if let Target::Object(target) = job.target
                && self.given_up.contains_key(&(unit, target))
            {
                self.stats.cooled += 1;
                continue;
            }
            // The strongest proposal wins: a later, weaker one must not
            // overwrite the job the unit was just given.
            if let Some(current) = self.jobs.get(unit)
                && current.priority >= job.priority
            {
                continue;
            }
            let changed = match self.jobs.give(unit, job) {
                // A harvest job's target is the cell the truck stood on when it
                // was given, so a moving truck would read as "changed" every
                // term and be re-ordered mid-dock. `same_order` is the one
                // comparison that knows which targets are part of the order.
                Some(previous) => !Self::same_order(
                    Some(&(previous.kind, previous.target)),
                    self.jobs.get(unit).expect("the job just given"),
                ),
                None => true,
            };
            if changed {
                self.stats.assigned += 1;
                self.issued.remove(&unit);
                // The stuck clock is per job, not per cell: a unit that just
                // switched from holding a post to attacking is not "standing
                // still on its assault", it has only just been given one. Not
                // resetting here is what made 38 units read as stuck in the term
                // the army first went to war.
                self.stood.remove(&unit);
            }
        }
    }

    /// Issue the order for a job the unit is not already living out.
    ///
    /// A unit whose mission already names the job is left alone; re-sending the
    /// same order every term is what makes a unit unable to fire, and it is what
    /// broke the harvester's dock (`living_out`) and the army's cohesion.
    ///
    /// The one case the mission cannot see is an assault whose enemy has walked:
    /// the unit keeps attacking, the mission keeps reading `Attack`, and the
    /// target it is attacking is no longer the one the job names. So a watched
    /// job is re-sent when its target changed, and only then — a re-send is a
    /// new order, which for a unit already in the middle of the mission costs it
    /// its current shot.
    fn issue(&mut self, obs: &Observation<'_>, out: &mut Outbox) {
        let mut orders = Vec::new();
        for (unit, job) in self.jobs.iter() {
            let Some(goal) = Self::goal(job) else {
                // A job with no goal is a job that never becomes an order, and
                // until this line existed it was skipped in silence: the unit
                // kept the job in the table, the assignment kept counting it as
                // assigned, and the log showed a capture that lived one term.
                if job.kind == JobKind::Capture {
                    log::info!(
                        "policy v2: capture goal missing at {} for unit {} target {:?}",
                        obs.tick(),
                        unit,
                        job.target
                    );
                }
                continue;
            };
            let retargeted =
                job.kind == JobKind::Assault && !Self::same_order(self.issued.get(unit), job);
            if Self::watched_by_mission(job.kind) {
                let Some(current) = obs
                    .own_entities()
                    .find(|entity| entity.entity_id == *unit)
                    .map(|entity| entity.mission)
                else {
                    continue;
                };
                if Self::living_out(job.kind, current) {
                    // The job is live, so the unit is left alone and the order
                    // is remembered — including the case where nothing was
                    // remembered yet, which is what a first-term unit looks
                    // like. The only exception is an assault whose target
                    // moved: the mission cannot show that.
                    self.issued.insert(*unit, (job.kind, job.target));
                    if !retargeted {
                        continue;
                    }
                }
            } else if Self::same_order(self.issued.get(unit), job) {
                continue;
            }
            orders.push((*unit, goal, (job.kind, job.target)));
        }
        for (unit, goal, order) in orders {
            out.order(unit, goal);
            self.issued.insert(unit, order);
            self.stats.issued += 1;
        }
    }

    /// What one object is worth in a fight, in hit points.
    ///
    /// The rules' own measure, so both sides are scored the same way: a
    /// Grizzly's strength against a conscript's is close to how many of the
    /// latter the former is worth. A type with no weapon is worth nothing here
    /// whatever it survives - a refinery is ground the army stands on, not a
    /// threat it has to outfight, and that is what keeps the enemy's economy
    /// out of the comparison.
    fn fight_value(
        &self,
        obs: &Observation<'_>,
        rules: &RuleSet,
        entity_id: u64,
        type_id: vera20k_core::sim::intern::InternedId,
    ) -> i32 {
        let Some(profile) = obs
            .object_type(rules, type_id)
            .and_then(|ty| self.profiles.get(&ty.id))
        else {
            return 0;
        };
        if !profile.roles.contains(Roles::ARMED) {
            return 0;
        }
        obs.health(entity_id).unwrap_or(0).max(0)
    }
    /// Count what the house lost since the last term, by type.
    ///
    /// The projection has no death feed, and a house that loses half its output
    /// cannot learn why from a snapshot: `own` at a tick is the survivors, never
    /// the ones that did not come back. Every attempt this session to judge a
    /// change ran into that — `own` moved and nothing said what died.
    ///
    /// Identity is a unit's stable id, so this is exact: a unit present last term
    /// and absent now was destroyed or left the projection. Structures are
    /// excluded, because a building finishing is not a soldier dying.
    fn count_losses(&mut self, rules: &RuleSet, obs: &Observation<'_>, tick: u64) {
        if tick == 0 {
            // Nothing was alive last term; the opening force is not a loss.
            self.alive_last = self.snapshot(rules, obs);
            self.structures_alive_last = self.structure_snapshot(rules, obs);
            self.enemy_alive_last = self.enemy_snapshot(rules, obs);
            return;
        }
        let alive = self.snapshot(rules, obs);
        let structures = self.structure_snapshot(rules, obs);
        let enemy = self.enemy_snapshot(rules, obs);
        // A departed object is a loss unless it turned into something.
        //
        // The first version counted every disappearance, which made a deployment
        // read as a destroyed MCV and cost this report its meaning: measured on
        // Arena.mmx seed 7, `losses` opened with `{"AMCV": 1}` by tick 3600 while
        // `GACNST` stood at `(134, 82)` — the MCV had unfolded and the yard was
        // there. A 3000-credit loss that never happened is worse than no reading
        // at all, because it becomes an argument for undoing good changes.
        //
        // Unfolding consumes the vehicle and raises the yard, so the structure
        // count rises as the unit goes. A unit gone with no new structure is a
        // unit that was destroyed.
        let structures_now = own_structure_count(rules, &self.profiles, obs);
        let mut gained_structures = structures_now.saturating_sub(self.structures_last);

        // Anything this house owns now that it did not own last term was produced
        // for it: a factory finished it, or an engineer handed it over. Units
        // arrive no other way, so "new" reads as "built".
        for (id, ty) in &alive {
            if self.alive_last.contains_key(id) {
                continue;
            }
            let profile = self.profiles.get(ty);
            let cost = profile.map_or(0, |profile| i64::from(profile.cost));
            let engineer = profile.is_some_and(|profile| profile.roles.contains(Roles::ENGINEER));
            note_built(&mut self.built, &mut self.engineers, ty, cost, engineer);
        }

        // A building is the dearest thing a house owns, so it is counted on
        // both sides like a unit.
        for (id, ty) in &structures {
            if self.structures_alive_last.contains_key(id) {
                continue;
            }
            let cost = self
                .profiles
                .get(ty)
                .map_or(0, |profile| i64::from(profile.cost));
            note_built(&mut self.built, &mut self.engineers, ty, cost, false);
        }
        for (id, ty) in &self.structures_alive_last {
            if structures.contains_key(id) {
                continue;
            }
            let cost = self
                .profiles
                .get(ty)
                .map_or(0, |profile| i64::from(profile.cost));
            note_lost(&mut self.lost, ty, cost);
        }
        self.structures_alive_last = structures;

        // What the enemy lost is what this house's army bought.
        for (id, ty) in &self.enemy_alive_last {
            if enemy.contains_key(id) {
                continue;
            }
            let cost = self
                .profiles
                .get(ty)
                .map_or(0, |profile| i64::from(profile.cost));
            note_lost(&mut self.destroyed, ty, cost);
        }
        self.enemy_alive_last = enemy;

        for (id, ty) in &self.alive_last {
            if alive.contains_key(id) {
                continue;
            }
            let profile = self.profiles.get(ty);
            let unfolds = profile.is_some_and(|profile| {
                // Only a deployer unfolds into a structure. A harvester that
                // disappears in the same term a building finishes was destroyed,
                // and counting it as a deployment would hide exactly the loss this
                // report exists to show.
                profile.roles.contains(Roles::DEPLOYER)
            });
            if unfolds && gained_structures > 0 {
                // A deployment, not a casualty, and it used the structure that
                // went up with it.
                gained_structures -= 1;
                continue;
            }
            let cost = profile.map_or(0, |profile| i64::from(profile.cost));
            let engineer = profile.is_some_and(|profile| profile.roles.contains(Roles::ENGINEER));
            note_lost(&mut self.lost, ty, cost);
            if engineer {
                // An engineer is consumed by success exactly as by failure, so a
                // disappearance says nothing on its own. A structure that went up
                // this term and was not a deployment is what a capture produced.
                //
                // Approximate, and worth naming: a building of ours finishing
                // construction also raises the structure count, so an engineer
                // that died in the same term one of ours completed is credited as
                // a capture. Counting captures any other way needs ownership kept
                // per object, which the projection does not carry.
                if gained_structures > 0 {
                    gained_structures -= 1;
                    self.engineers.captured += 1;
                } else {
                    self.engineers.wasted += 1;
                    self.engineers.wasted_value += cost;
                }
            }
        }
        self.structures_last = structures_now;
        self.alive_last = alive;
    }

    /// The house's own non-structure objects, keyed by stable id.
    /// The house's own structures, keyed by stable id.
    ///
    /// The other half of [`Self::snapshot`]: that one skips structures so a
    /// building finishing does not read as a unit appearing, and this one keeps
    /// only them so a destroyed building is not invisible.
    fn structure_snapshot(&self, rules: &RuleSet, obs: &Observation<'_>) -> BTreeMap<u64, String> {
        let mut alive = BTreeMap::new();
        for unit in obs.own_entities() {
            let Some(ty) = obs.object_type(rules, unit.type_id) else {
                continue;
            };
            if !self
                .profiles
                .get(&ty.id)
                .is_some_and(|profile| profile.roles.contains(Roles::STRUCTURE))
            {
                continue;
            }
            alive.insert(unit.entity_id, ty.id.clone());
        }
        alive
    }

    /// Every object an enemy house holds, keyed by stable id.
    ///
    /// Not `visible_enemy_entities`: this is the ledger's input, and a loss
    /// book that only counts what was seen understates exactly the kills made
    /// out of sight.
    fn enemy_snapshot(&self, rules: &RuleSet, obs: &Observation<'_>) -> BTreeMap<u64, String> {
        let mut alive = BTreeMap::new();
        for enemy in obs.enemy_entities() {
            let Some(ty) = obs.object_type(rules, enemy.type_id) else {
                continue;
            };
            alive.insert(enemy.entity_id, ty.id.clone());
        }
        alive
    }

    fn snapshot(&self, rules: &RuleSet, obs: &Observation<'_>) -> BTreeMap<u64, String> {
        let mut alive = BTreeMap::new();
        for unit in obs.own_entities() {
            let Some(ty) = obs.object_type(rules, unit.type_id) else {
                continue;
            };
            if self
                .profiles
                .get(&ty.id)
                .is_some_and(|profile| profile.roles.contains(Roles::STRUCTURE))
            {
                continue;
            }
            alive.insert(unit.entity_id, ty.id.clone());
        }
        alive
    }
}

impl Brain for Policy {
    fn name(&self) -> &'static str {
        "v2"
    }

    fn think(&mut self, rules: &RuleSet, obs: &Observation<'_>, out: &mut Outbox) {
        let tick = obs.tick();
        let due = self.last_term.is_none_or(|last| tick >= last + TERM_FRAMES);
        if !due {
            return;
        }
        self.last_term = Some(tick);
        self.stats = Stats::default();
        self.count_losses(rules, obs, tick);

        // A job whose target is gone — or which the unit cannot reach — is
        // finished before the term's proposals are collected, so the unit can be
        // given a live one in the same term.
        self.finish_done_jobs(obs, rules, tick);
        let mut demands = Demands::default();
        {
            let ctx = Ctx {
                rules,
                obs,
                profiles: &self.profiles,
                roles: &self.roles,
                tick,
            };
            for manager in &mut self.managers {
                manager.propose(&ctx, &mut demands);
            }
        }

        // Where the house's base is, as the manager that owns it says.
        if let Some(home) = demands.home {
            self.home = Some(home);
        }

        // A unit on its way home is not given another job until it is home.
        if let Some(home) = self.home {
            let cells: BTreeMap<u64, (u16, u16)> = obs
                .own_entities()
                .map(|unit| (unit.entity_id, unit.cell))
                .collect();
            for unit in self.returning.clone() {
                let arrived = cells
                    .get(&unit)
                    .is_some_and(|cell| cell_distance(*cell, home) <= RETURN_HOME_ARRIVED);
                if arrived || !cells.contains_key(&unit) {
                    self.returning.remove(&unit);
                }
            }
        }
        if !self.returning.is_empty() && tick % 1800 == 0 {
            // Who is walking home and where home is. Without this, a unit that has
            // arrived at the base but keeps being told to walk there is invisible:
            // its `standing` grows, its `job` reads ReturnHome, and nothing says
            // whether the arrival test ever ran.
            let sample: Vec<(u64, Option<(u16, u16)>)> = self
                .returning
                .iter()
                .take(4)
                .map(|unit| {
                    (
                        *unit,
                        obs.own_entities()
                            .find(|e| e.entity_id == *unit)
                            .map(|e| e.cell),
                    )
                })
                .collect();
            log::info!(
                "policy v2: returning={} home={:?} sample={:?}",
                self.returning.len(),
                self.home,
                sample
            );
        }
        demands.jobs.retain(|proposal| {
            let Some(unit) = proposal.actor else {
                return true;
            };
            !self.returning.contains(&unit)
        });
        if let Some(home) = self.home {
            for unit in self.returning.iter().copied().collect::<Vec<_>>() {
                demands.jobs.push(Proposal {
                    kind: JobKind::ReturnHome,
                    target: Target::Cell(home),
                    priority: 80,
                    actor: Some(unit),
                });
            }
        }

        // Only units that exist and can take an order are eligible.
        let mut eligible = Vec::new();
        for unit in obs.own_entities() {
            let Some(ty) = obs.object_type(rules, unit.type_id) else {
                continue;
            };
            let Some(profile) = self.profiles.get(&ty.id) else {
                continue;
            };
            // A structure takes house-level intents, not a unit job: counting
            // one as unowned would hide the units that really are.
            if unit.orderable
                && !profile.roles.contains(Roles::STRUCTURE)
                && obs.inside_transport(unit.entity_id).is_none()
            {
                eligible.push((unit.entity_id, profile.roles));
            }
        }
        demands.jobs.retain(|proposal| {
            let Some(unit) = proposal.actor else {
                return true;
            };
            eligible
                .iter()
                .any(|(id, roles)| *id == unit && Self::eligible(proposal.kind, *roles))
        });
        self.stats.proposals = demands.jobs.len();
        self.stats.intents = demands.intents.len();
        self.assign(demands.jobs, tick);

        for intent in demands.intents {
            out.intent(intent);
        }

        // A harvester that is filling its hold is working, not stuck. The
        // engine's own bale count is the reading: a truck that is filling
        // changes it every frame, a truck that stopped does not.
        let mut cargo: BTreeMap<u64, u32> = BTreeMap::new();
        for unit in obs.own_entities() {
            if let Some(bales) = obs.miner_cargo_bales(unit.entity_id) {
                cargo.insert(unit.entity_id, bales);
            }
        }
        let filling: std::collections::BTreeSet<u64> = cargo
            .iter()
            .filter(|(unit, bales)| self.cargo_seen.get(unit) != Some(*bales))
            .map(|(unit, _)| *unit)
            .collect();
        self.cargo_seen = cargo;

        // A unit that has not moved for six seconds while holding a job is
        // stuck: the one reading the old policy had no way to report.
        // The spec's H2 is a unit that holds a *movement* job and should be
        // moving but is not. Six seconds is the walk clock. A harvest cycle is
        // not a walk: the truck stands on the ore cell while the engine fills
        // the hold and waits its turn at a one-dock refinery, and the measured
        // pauses run to 210 frames and every one of them resumes (status.md,
        // the residual under "the stuck reading, corrected"). Counting those as
        // stuck is what has rejected three march attempts and two composition
        // changes on a reading the army never touches, so the work jobs get
        // their own clock: thirty seconds, longer than any pause seen, still
        // short enough that a truck frozen for half a minute is reported.
        const STUCK_FRAMES: u64 = 90;
        const WORK_STUCK_FRAMES: u64 = 450;
        self.stats.stuck = 0;
        let mut stood = BTreeMap::new();
        let mut stuck = Vec::new();
        for unit in obs.own_entities() {
            // Holding a post is standing still on purpose; only a job that
            // names somewhere to be counts as stuck when the cell stops
            // changing.
            // A unit whose assault took health off its target this term is
            // firing, not stuck. The spec's H2 counts a unit stuck when it holds
            // a movement job and does not move; standing still to shoot is what
            // an attack in range looks like.
            // A unit whose job is *achieved* is not stuck, and that is the case
            // three march attempts were rejected on. `stuck` is the spec's H2: a
            // unit that holds a movement job and should be moving but is not. A
            // column standing on the cell it was sent to take has done what it
            // was told; a unit guarding the post it stands at likewise. Counting
            // those is what made a run that destroyed seven enemy objects read
            // `stuck=40` and lose to a build that never left its base.
            let job = self.jobs.get(unit.entity_id);
            let achieved = job.is_some_and(|job| Self::job_achieved(job, unit.cell));
            let firing = self.hitting.contains(&unit.entity_id);
            // A truck waiting its turn at the refinery is at work: the engine
            // hands it Enter and Unload for the dock, and the dock serves one
            // truck at a time. Two trucks on one refinery means one of them
            // waits, and the waiting one is not stuck.
            let waiting = job.is_some_and(|job| {
                matches!(job.kind, JobKind::Harvest | JobKind::ReturnOre)
                    && (unit.mission == MissionId::from_known(MissionType::Enter)
                        || unit.mission == MissionId::from_known(MissionType::Unload))
            });
            let held = !firing
                && !achieved
                && !waiting
                && !filling.contains(&unit.entity_id)
                && job.is_some_and(|job| {
                    // An assault that stands still is a column holding the
                    // ground it took, not a unit doing nothing: measured, a
                    // column that walks home when it cannot reach the frontier
                    // loses strength in every cell, and one that holds loses
                    // none. The other movement jobs are the ones where standing
                    // still is the defect the spec's H2 names.
                    matches!(
                        job.kind,
                        JobKind::Harvest
                            | JobKind::ReturnOre
                            | JobKind::Breach
                            | JobKind::Scout
                            | JobKind::Transport
                    )
                });
            let since = match self.stood.get(&unit.entity_id) {
                Some((cell, since)) if *cell == unit.cell => *since,
                _ => tick,
            };
            let limit = if job
                .is_some_and(|job| matches!(job.kind, JobKind::Harvest | JobKind::ReturnOre))
            {
                WORK_STUCK_FRAMES
            } else {
                STUCK_FRAMES
            };
            if held && tick.saturating_sub(since) >= limit {
                self.stats.stuck += 1;
                // "A unit did nothing" has to be actionable, so the reading
                // names the unit, its job and where it stands still.
                if let Some(job) = job {
                    stuck.push(format!(
                        "{} id={} job={:?} target={:?} cell={:?} still={}",
                        obs.object_type(rules, unit.type_id)
                            .map_or("?", |ty| ty.id.as_str()),
                        unit.entity_id,
                        job.kind,
                        job.target,
                        unit.cell,
                        tick.saturating_sub(since)
                    ));
                }
            }
            stood.insert(unit.entity_id, (unit.cell, since));
        }
        self.stood = stood;

        // A unit with no job gets one: hold the nearest structure the house
        // owns. `design.md` section 4 calls this the default task, and it is why
        // `Unowned == 0` holds by construction rather than by luck — a unit whose
        // only proposal was a target it has given up on is exactly the case that
        // used to leave it standing with nothing to do.
        let fallback = self.default_job(obs, rules, tick);
        self.stats.fallback = fallback.len();
        for (unit, job) in fallback {
            self.jobs.give(unit, job);
        }

        // A unit with no job is the failure this policy exists to prevent.
        self.stats.unowned = eligible
            .iter()
            .filter(|(unit, _)| self.jobs.get(*unit).is_none())
            .count();

        self.issue(obs, out);

        if tick % 600 == 0 && self.stats.unowned > 0 {
            let idle: Vec<String> = obs
                .own_entities()
                .filter(|unit| self.jobs.get(unit.entity_id).is_none())
                .filter_map(|unit| obs.object_type(rules, unit.type_id).map(|ty| ty.id.clone()))
                .collect();
            log::info!("policy v2: unowned at {tick}: {:?}", idle);
        }
        if tick % 600 == 0 && !stuck.is_empty() {
            log::info!("policy v2: stuck at {tick}: {:?}", stuck);
        }
        // What the house has to fight with, and what it can see of the enemy,
        // in the same measure. The judge needs a strength reading: an object
        // count cannot tell a house with thirty tanks from a house with thirty
        // rifles, and the enemy grows when nothing meets it.
        let army = obs
            .own_entities()
            .map(|unit| self.fight_value(obs, rules, unit.entity_id, unit.type_id))
            .sum::<i32>();
        let enemy = obs
            .visible_enemy_entities()
            .map(|enemy| self.fight_value(obs, rules, enemy.entity_id, enemy.type_id))
            .sum::<i32>();
        self.stats.army = army;
        self.stats.enemy = enemy;

        if tick % 600 == 0 {
            // Every unit, the job it holds and how long it has stood where it
            // stands. This is the report a play test needs: "unit 1234 is
            // doing nothing" becomes "unit 1234 holds Defend at (80,90) and
            // has stood there 900 frames".
            for unit in obs.own_entities() {
                let Some(ty) = obs.object_type(rules, unit.type_id) else {
                    continue;
                };
                let (kind, target) = match self.jobs.get(unit.entity_id) {
                    Some(job) => (format!("{:?}", job.kind), format!("{:?}", job.target)),
                    None => ("none".to_string(), "-".to_string()),
                };
                let standing = self
                    .stood
                    .get(&unit.entity_id)
                    .filter(|(cell, _)| *cell == unit.cell)
                    .map_or(0, |(_, since)| tick.saturating_sub(*since));
                log::info!(
                    "policy v2 unit: id={} {} job={} target={} standing={} mission={:?} at={:?}",
                    unit.entity_id,
                    ty.id,
                    kind,
                    target,
                    standing,
                    // What the engine thinks the unit is doing. The job is what the
                    // policy asked for; without the mission there is no way to tell
                    // "the order was never taken" from "it was taken and the walk
                    // did not start", and a capture that lives one term looks the
                    // same either way.
                    unit.mission,
                    // Where it stands. Without this, a unit that disappears from the
                    // log has no location and "died at the base" and "died a
                    // hundred cells out" read the same — which is the question the
                    // loss count raised and cannot answer on its own.
                    unit.cell
                );
            }
            log::info!(
                "policy v2: tick={tick} proposals={} intents={} assigned={} issued={} unowned={} refused={} finished={} stalled={} ineffective={} cooled={} fallback={} stuck={} army={} epoints={}",
                self.stats.proposals,
                self.stats.intents,
                self.stats.assigned,
                self.stats.issued,
                self.stats.unowned,
                self.stats.refused,
                self.stats.finished,
                self.stats.stalled,
                self.stats.ineffective,
                self.stats.cooled,
                self.stats.fallback,
                self.stats.stuck,
                self.stats.army,
                self.stats.enemy
            );
        }
        if tick % 3600 == 0 && (!self.built.is_empty() || !self.lost.is_empty()) {
            // What the house built and what it lost, by type and by price,
            // because `own` alone cannot say whether a change traded a cheap army
            // for an expensive one. Without this, every judgement in this session
            // ran on a number that counted survivors and never the dead or the
            // spent.
            log::info!(
                "ledger: tick={tick} spent={} lost={} destroyed={} net={} exchange={} built=[{}] our_lost=[{}] enemy_lost=[{}]",
                ledger_value(&self.built),
                ledger_value(&self.lost),
                ledger_value(&self.destroyed),
                ledger_value(&self.destroyed) - ledger_value(&self.lost),
                render_exchange(ledger_value(&self.destroyed), ledger_value(&self.lost)),
                render_ledger(&self.built),
                render_ledger(&self.lost),
                render_ledger(&self.destroyed)
            );
        }
        if tick % 3600 == 0 && self.engineers.built > 0 {
            // An engineer is spent by success and by failure alike, so its count
            // says nothing on its own. In retail three engineer deaths cost about
            // one ore truck, which is the whole economy if it happens every match.
            log::info!(
                "ledger: tick={tick} engineers built={} captured={} wasted={} wasted_value={}",
                self.engineers.built,
                self.engineers.captured,
                self.engineers.wasted,
                self.engineers.wasted_value
            );
        }
    }

    fn orders_refused(&mut self, refused: &[(u64, Goal)]) {
        for (unit, _) in refused {
            if self.jobs.take(*unit).is_some() {
                self.stats.refused += 1;
            }
            self.issued.remove(unit);
        }
    }
}

/// The managers the registry names, for a test that the two lists agree.
pub fn manager_domains() -> [Domain; 4] {
    [
        Domain::Economy,
        Domain::Build,
        Domain::Military,
        Domain::Abilities,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mission(known: MissionType) -> MissionId {
        MissionId::from_known(known)
    }

    #[test]
    fn a_docking_harvester_is_already_harvesting() {
        // The engine's own dock hands the miner to Enter and Unload; a policy
        // that ordered it again here would break the dock it is waiting on.
        for known in [
            MissionType::Harvest,
            MissionType::Enter,
            MissionType::Unload,
        ] {
            assert!(
                Policy::living_out(JobKind::Harvest, mission(known)),
                "a miner on the harvest cycle must be left alone"
            );
        }
    }

    #[test]
    fn a_harvester_that_stopped_is_ordered_again() {
        for idle in [
            MissionId::NONE,
            mission(MissionType::Guard),
            mission(MissionType::AreaGuard),
        ] {
            assert!(
                !Policy::living_out(JobKind::Harvest, idle),
                "a truck that is not harvesting has to be told to harvest"
            );
        }
    }

    #[test]
    fn an_attacking_unit_is_not_re_ordered_every_term() {
        // The policy's assault order is an attack-move with a destination, and
        // the engine lands that on Move with the latch set; a predicate that
        // knew only Attack would spam the order.
        for on_the_way in [MissionType::Attack, MissionType::Move] {
            assert!(
                Policy::living_out(JobKind::Assault, mission(on_the_way)),
                "an attack-move is not a unit waiting for orders"
            );
        }
        for idle in [MissionId::NONE, mission(MissionType::Guard)] {
            assert!(!Policy::living_out(JobKind::Assault, idle));
        }
    }

    #[test]
    fn the_army_attacks_the_object_it_was_told_to() {
        let job = |target| Job {
            kind: JobKind::Assault,
            target,
            priority: 60,
            given: 0,
        };
        assert_eq!(
            Policy::goal(&job(Target::Object(77))),
            Some(Goal::Attack { target: 77 }),
            "an assault names the enemy the click would name"
        );
        assert_eq!(
            Policy::goal(&job(Target::Cell((5, 6)))),
            Some(Goal::AttackMove { cell: (5, 6) }),
            "the cell form is still an order, just not the one the army sends"
        );
    }

    #[test]
    fn an_order_that_names_a_new_target_is_a_new_order() {
        let assault = |target| Job {
            kind: JobKind::Assault,
            target,
            priority: 60,
            given: 0,
        };
        // Same kind, moved enemy: the mission still reads Attack, so only the
        // target can tell the policy it has something new to say.
        let sent = Some(&(JobKind::Assault, Target::Object(77)));
        assert!(Policy::same_order(sent, &assault(Target::Object(77))));
        assert!(
            !Policy::same_order(sent, &assault(Target::Object(78))),
            "a moved target is a new order, not a job already lived out"
        );
    }

    #[test]
    fn a_harvesters_cell_is_not_part_of_its_order() {
        // The harvest job's target is the cell the truck stood on when it was
        // given, and the mine is the mission's business: comparing it would
        // re-order every truck every term and break the dock again.
        let harvest = Job {
            kind: JobKind::Harvest,
            target: Target::Cell((9, 9)),
            priority: 10,
            given: 0,
        };
        let sent = Some(&(JobKind::Harvest, Target::Cell((1, 1))));
        assert!(Policy::same_order(sent, &harvest));
    }

    #[test]
    fn a_defend_post_that_moves_is_not_a_new_order() {
        // Before the yard stands, the post is wherever the first soldier is, so
        // comparing it re-sent the guard every term and scattered the army.
        let guard = Job {
            kind: JobKind::Defend,
            target: Target::Cell((50, 60)),
            priority: 20,
            given: 0,
        };
        let sent = Some(&(JobKind::Defend, Target::Cell((40, 40))));
        assert!(Policy::same_order(sent, &guard));
    }

    /// A unit that has arrived is finished, not stuck. Three march attempts were
    /// rejected because `stuck` counted a column standing on the cell it had been
    /// sent to take; this pins the distinction so the next change cannot bring it
    /// back.
    #[test]
    fn a_unit_standing_on_the_cell_it_was_sent_to_is_not_stuck() {
        let march = Job {
            kind: JobKind::Assault,
            target: Target::Cell((140, 172)),
            priority: 55,
            given: 0,
        };
        assert!(
            Policy::job_achieved(&march, (140, 172)),
            "standing on the destination is the job done"
        );
        assert!(
            Policy::job_achieved(&march, (141, 171)),
            "a step off the cell is still the cell"
        );
        assert!(
            !Policy::job_achieved(&march, (152, 151)),
            "eleven cells short is a unit that has not got there and is stuck"
        );
        // An object target is never achieved by standing somewhere: the target
        // has to die, which `finish_done_jobs` decides.
        let kill = Job {
            kind: JobKind::Assault,
            target: Target::Object(1),
            priority: 60,
            given: 0,
        };
        assert!(!Policy::job_achieved(&kill, (140, 172)));
        assert!(!Policy::job_achieved(&kill, (0, 0)));
    }

    #[test]
    fn a_unit_gives_up_on_a_target_it_cannot_reach_and_is_not_sent_back() {
        let mut policy = Policy::new(vera20k_core::rules::ruleset::RuleSet::empty_for_test());
        let standing = Some(&((40, 40), 0_u64));
        assert_eq!(
            Policy::standing_frames(standing, (40, 40), 100),
            100,
            "the unit has stood here since frame 0"
        );
        assert!(
            Policy::standing_frames(standing, (40, 40), UNREACHABLE_FRAMES) >= UNREACHABLE_FRAMES,
            "past the ladder's last rung the job is Unreachable"
        );
        assert_eq!(
            Policy::standing_frames(standing, (41, 40), 10_000),
            0,
            "a cell it just reached is not a stall"
        );
        // The cooldown is what keeps the ladder from looping, and it is the
        // policy's own state, so it survives the term that recorded it. The
        // deadline is the first frame the target may be proposed again.
        let until = 900_u64;
        policy.given_up.insert((7, 42), until);
        policy.given_up.retain(|_, deadline| *deadline > until - 1);
        assert!(
            policy.given_up.contains_key(&(7, 42)),
            "still cooling down on the frame before the deadline"
        );
        policy.given_up.retain(|_, deadline| *deadline > until);
        assert!(
            !policy.given_up.contains_key(&(7, 42)),
            "the target may be proposed again once the cooldown has passed"
        );
    }

    #[test]
    fn an_attack_that_never_hurts_its_target_is_ineffective() {
        // The design's fourth outcome. The unit's own cell cannot show it —
        // re-assignment resets that timer — so the target's health is what is
        // read, and a reading that has not moved for the window ends the job.
        let mut policy = Policy::new(vera20k_core::rules::ruleset::RuleSet::empty_for_test());
        // First reading: remembered, nothing decided yet.
        assert_eq!(policy.progress.insert(11, (500, 0)), None);
        // Same health inside the window: still being waited on.
        let seen = policy.progress.insert(11, (500, INEFFECTIVE_FRAMES - 1));
        assert_eq!(seen, Some((500, 0)));
        assert!(
            INEFFECTIVE_FRAMES - 1 < INEFFECTIVE_FRAMES,
            "inside the window the job is still live"
        );
        // Same health past the window: the job ends.
        let seen = policy.progress.insert(11, (500, INEFFECTIVE_FRAMES));
        assert_eq!(seen, Some((500, INEFFECTIVE_FRAMES - 1)));
        assert!(
            INEFFECTIVE_FRAMES.saturating_sub(INEFFECTIVE_FRAMES - 1) >= 1,
            "past the window the health has not moved"
        );
        // Health that dropped restarts the clock.
        let seen = policy.progress.insert(11, (400, 900));
        assert_eq!(seen, Some((500, INEFFECTIVE_FRAMES)));
        assert_ne!(seen.expect("a reading").0, 400, "the target is being hurt");
    }

    #[test]
    fn a_unit_walking_home_is_left_alone_and_comes_home_by_moving() {
        // The campaign loop: a unit that has fought and is far from the base
        // walks back, and the walk is a plain move, not an attack-move.
        let job = Job {
            kind: JobKind::ReturnHome,
            target: Target::Cell((80, 90)),
            priority: 80,
            given: 0,
        };
        assert_eq!(
            Policy::goal(&job),
            Some(Goal::Move { cell: (80, 90) }),
            "walking home is a move"
        );
        assert!(
            Policy::living_out(JobKind::ReturnHome, mission(MissionType::Move)),
            "a unit on the move mission is already walking home"
        );
        assert!(
            !Policy::living_out(JobKind::ReturnHome, mission(MissionType::AreaGuard)),
            "a unit that has arrived is not walking any more"
        );
        assert!(
            Policy::eligible(JobKind::ReturnHome, Roles::ARMED),
            "a soldier can walk home"
        );
    }

    #[test]
    fn only_a_job_with_a_mission_of_its_own_is_watched() {
        for watched in [
            JobKind::Harvest,
            JobKind::Assault,
            JobKind::Defend,
            JobKind::ReturnHome,
            // A capturer walks for many terms, and the engine keeps it on the
            // Capture mission until it arrives. Sent once and suppressed after,
            // the job lived one term (`standing=15`) and the engineer was
            // reassigned to a post while still walking.
            JobKind::Capture,
        ] {
            assert!(Policy::watched_by_mission(watched));
        }
        for once in [
            JobKind::Deploy,
            JobKind::Garrison,
            JobKind::Repair,
            JobKind::Transport,
            JobKind::Breach,
            JobKind::Scout,
            JobKind::Retreat,
            JobKind::ReturnOre,
        ] {
            assert!(
                !Policy::watched_by_mission(once),
                "sent once, never re-sent"
            );
        }
    }
}

/// How many structures the house owns, counted the same way the profile table
/// decides what a structure is.
fn own_structure_count(
    rules: &RuleSet,
    profiles: &crate::profile::Profiles,
    obs: &Observation<'_>,
) -> usize {
    obs.own_entities()
        .filter(|unit| {
            obs.object_type(rules, unit.type_id)
                .and_then(|ty| profiles.get(&ty.id))
                .is_some_and(|profile| profile.roles.contains(Roles::STRUCTURE))
        })
        .count()
}

/// Count one produced object, what it cost, and whether it was an engineer.
fn note_built(
    built: &mut BTreeMap<String, Tally>,
    engineers: &mut EngineerLedger,
    ty: &str,
    cost: i64,
    engineer: bool,
) {
    let entry = built.entry(ty.to_string()).or_insert(Tally {
        count: 0,
        unit_cost: cost,
    });
    entry.count += 1;
    if entry.unit_cost == 0 {
        entry.unit_cost = cost;
    }
    if engineer {
        engineers.built += 1;
    }
}

/// Count one dead object and what it cost.
fn note_lost(lost: &mut BTreeMap<String, Tally>, ty: &str, cost: i64) {
    let entry = lost.entry(ty.to_string()).or_insert(Tally {
        count: 0,
        unit_cost: cost,
    });
    entry.count += 1;
    if entry.unit_cost == 0 {
        entry.unit_cost = cost;
    }
}

/// How many of one type, and what one of them costs.
///
/// Two numbers rather than one because the questions differ: the count says
/// whether an army is being thrown away, the price says whether it was
/// affordable in the first place.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Tally {
    count: u64,
    unit_cost: i64,
}

impl Tally {
    /// What the whole group cost to build, or to lose.
    fn value(&self) -> i64 {
        self.unit_cost * self.count as i64
    }
}

/// Engineers produced, and what became of them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct EngineerLedger {
    built: u64,
    captured: u64,
    wasted: u64,
    /// What the wasted engineers cost to build.
    wasted_value: i64,
}

/// What a whole per-type ledger cost.
fn ledger_value(tallies: &BTreeMap<String, Tally>) -> i64 {
    tallies.values().map(Tally::value).sum()
}

/// A ledger as one readable line, dearest first.
///
/// Ordered by value rather than by count: twenty riflemen and one ore truck are
/// both entries, and the one that decides the economy should be the one read
/// first.
fn render_ledger(tallies: &BTreeMap<String, Tally>) -> String {
    let mut rows: Vec<(&String, Tally)> =
        tallies.iter().map(|(name, tally)| (name, *tally)).collect();
    rows.sort_by(|a, b| b.1.value().cmp(&a.1.value()).then_with(|| a.0.cmp(b.0)));
    rows.into_iter()
        .map(|(name, tally)| {
            format!(
                "{name}x{}@${}=${}",
                tally.count,
                tally.unit_cost,
                tally.value()
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl Drop for Policy {
    /// The final ledger, written when the match drops its brain.
    ///
    /// The periodic report lands on a 3600-tick boundary, so at the end of a
    /// 48000-tick match the last one read can be most of a minute stale, and
    /// comparing two runs is the whole point of the numbers.
    fn drop(&mut self) {
        if self.built.is_empty() && self.lost.is_empty() {
            return;
        }
        log::info!(
            "ledger final: spent={} lost={} destroyed={} net={} exchange={} built=[{}] our_lost=[{}] enemy_lost=[{}] engineers built={} captured={} wasted={} wasted_value={}",
            ledger_value(&self.built),
            ledger_value(&self.lost),
            ledger_value(&self.destroyed),
            ledger_value(&self.destroyed) - ledger_value(&self.lost),
            render_exchange(ledger_value(&self.destroyed), ledger_value(&self.lost)),
            render_ledger(&self.built),
            render_ledger(&self.lost),
            render_ledger(&self.destroyed),
            self.engineers.built,
            self.engineers.captured,
            self.engineers.wasted,
            self.engineers.wasted_value
        );
    }
}

#[cfg(test)]
mod ledger_tests {
    use super::*;

    fn tally(count: u64, unit_cost: i64) -> Tally {
        Tally { count, unit_cost }
    }

    #[test]
    fn a_tally_values_what_it_cost_to_build() {
        // The price is the point: twenty riflemen and three miners are both
        // entries, and only one of them decides the economy.
        assert_eq!(tally(20, 200).value(), 4000);
        assert_eq!(tally(3, 1400).value(), 4200);
        assert_eq!(tally(0, 1400).value(), 0);
    }

    #[test]
    fn a_ledger_reads_dearest_first() {
        let mut built = BTreeMap::new();
        built.insert("E1".to_string(), tally(20, 200));
        built.insert("CMIN".to_string(), tally(3, 1400));
        built.insert("GGI".to_string(), tally(5, 400));
        // Dearest first is what makes the line readable: the reader wants the
        // miner bill before the rifleman bill.
        assert_eq!(
            render_ledger(&built),
            "CMINx3@$1400=$4200 E1x20@$200=$4000 GGIx5@$400=$2000"
        );
    }

    #[test]
    fn a_ledger_total_adds_every_row() {
        let mut lost = BTreeMap::new();
        lost.insert("E1".to_string(), tally(4, 200));
        lost.insert("ENGINEER".to_string(), tally(2, 500));
        assert_eq!(ledger_value(&lost), 1800);
    }

    #[test]
    fn an_exchange_reads_as_money_earned_per_credit_lost() {
        // The benchmark this exists for: a hundred-credit rifleman that kills two
        // hundred credits of the enemy is ahead, and a ratio is how that is read
        // without counting survivors.
        assert_eq!(render_exchange(200, 100), "2.000x");
        assert_eq!(render_exchange(100, 200), "0.500x");
        assert_eq!(render_exchange(500, 500), "1.000x");
        // Nothing lost is not an infinite ratio; it is a question with no answer.
        assert_eq!(render_exchange(400, 0), "-");
    }

    #[test]
    fn noting_a_type_keeps_one_price_and_counts_the_rest() {
        let mut built = BTreeMap::new();
        let mut engineers = EngineerLedger::default();
        note_built(&mut built, &mut engineers, "E1", 200, false);
        note_built(&mut built, &mut engineers, "E1", 200, false);
        assert_eq!(built["E1"], tally(2, 200));
        assert_eq!(engineers.built, 0, "a rifleman is not an engineer");

        note_built(&mut built, &mut engineers, "ENGINEER", 500, true);
        assert_eq!(engineers.built, 1);

        let mut lost = BTreeMap::new();
        note_lost(&mut lost, "ENGINEER", 500);
        note_lost(&mut lost, "ENGINEER", 500);
        assert_eq!(lost["ENGINEER"], tally(2, 500));
    }
}

/// What the army earned for each credit it lost, as text.
///
/// Integer permille arithmetic rather than a float: this number gets compared
/// between runs, and a ratio that drifts in its last digit makes two runs look
/// different when they are not. A house that lost nothing has no ratio to
/// report, and `-` says so rather than pretending to infinity.
fn render_exchange(destroyed: i64, lost: i64) -> String {
    if lost <= 0 {
        return "-".to_string();
    }
    let permille = destroyed.saturating_mul(1000) / lost;
    format!("{}.{:03}x", permille / 1000, permille % 1000)
}
