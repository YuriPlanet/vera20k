//! Area `movement` of the bench (see `lib.rs`). Marks are named `movement.<point>`.
//!
//! The production, command, movement, path and unit AI decisions, and the
//! `Push` and `SetFacing` actions. Each point is a small state machine run
//! from `frame`, answered in `decide` and checked on a later frame (or from
//! a delivered `Commanded`, `Deployed` or `OrderRefused`), on objects this
//! area creates in its arena:
//! - player tanks (`MTNK`): `MoveFrame` then `Push` then `SetFacing` on one,
//!   `PathFound`, `IdleMission`, `MissionStep` and `ApproachPoint` (an
//!   attack on a sleeping computer GI) on one each, driven with
//!   `Action::set_mission`. Their decisions come in the units' own turns,
//!   outside this module's call (a decision raised inside the module's own
//!   call, such as the receiver of a `SetMission` write, skips the module's
//!   links), so the module's write is only the order;
//! - computer-house objects, ordered through `House::order` (queued, run in
//!   the next frame's command step, so `CommandAllowed` and `DestinationSet`
//!   reach this module): a Chrono Miner for `CommandAllowed` and
//!   `DestinationSet`, a Siege Chopper on level ground for `DeployAllowed`,
//!   a standing GI for `DeployInfantry` (its deploy command, the Unload
//!   mission); each removed once done;
//! - a computer MCV in the computer's own base, on a level 5x5 spot whose
//!   corner `find` answers `Placeable` for its yard type: `DeployMcv` (its
//!   `DeployAllowed` refusal, then the deploy and its `Deployed`) and
//!   `BirthYard` (the yard's `Birth`, `other` the MCV), once `HarvestCell`
//!   is done (or late in the run). The house holds a
//!   `Forces` claim from the allowed order until the deploy: a computer
//!   yard's deploy regroups the house's ground forces otherwise. Yard and MCV
//!   are removed afterwards;
//! - a player power plant destroyed by `Action::damage` (defenses read, so
//!   it leaves survivors: one roll in three per foundation cell, retried on
//!   a new plant up to eight times): `BirthSurvivor` (a crewman's `Birth`,
//!   `other` the plant); each survivor is removed;
//! - the computer house's own harvesters (`HarvestCell`) and production
//!   (`ProductionQueued` on the first type its factories start; the house
//!   gets 10000 credits early so they keep starting things);
//! - `ProductionAllowed` on the module's own `QueueProduction` for the
//!   computer house, once `ProductionQueued`, `HarvestCell` and the MCV
//!   scene are done (or late in the run): the house's
//!   `Production` is claimed for the scene (its AI starts nothing, and a
//!   house order fills the factory a player's would), a conscript (else a
//!   Rhino, else a power plant) is forbidden, queued (it must come back
//!   `OrderRefused` `Prerequisite` or at least not get under way), allowed
//!   back with its native answer and queued again (its factory's `Type`
//!   must read it with `Progress` started); then cancelled and the claim
//!   released.
//!
//! A `MoveFrame` decider that never answers (the observer) reads the NavCom
//! a `DestinationSet`, `ApproachPoint` or `HarvestCell` answer set.
//!
//! Not in `MARKS`:
//! - `ROTMultiplier` (the ROT modifier): `basics` covers it.
//!
//! Most likely to miss in the scene: the production pair needs the Easy
//! computer house's factories to start a bound type twice, and another one
//! while it is forbidden;
//! `HarvestCell` needs a computer harvester scanning on an ore field of at
//! least two cells' spread; the chopper's deploy needs it to land level;
//! the MCV needs a `Placeable` 5x5 spot in the computer's base.

use core::sync::atomic::{AtomicI64, Ordering::Relaxed};

use vera20k_sdk::prelude::*;
use vera20k_sdk::{
    DeployAllowedDecision, DestinationSetDecision, HarvestCellDecision, IdleMissionDecision,
    MissionStepDecision, NO_HANDLE, get, set, sys,
};

use crate::{alive, arena, bench_fail, enemy, mark, offset, player, spawn};

/// Every mark this area reaches in the bench's scene.
#[allow(dead_code)]
pub const MARKS: &[&str] = &[
    "movement.ProductionAllowed",
    "movement.ProductionQueued",
    "movement.CommandAllowed",
    "movement.DestinationSet",
    "movement.HarvestCell",
    "movement.DeployAllowed",
    "movement.DeployInfantry",
    "movement.DeployMcv",
    "movement.BirthYard",
    "movement.BirthSurvivor",
    "movement.IdleMission",
    "movement.MissionStep",
    "movement.MoveFrame",
    "movement.PathFound",
    "movement.ApproachPoint",
    "movement.Push",
    "movement.SetFacing",
];

// ------------------------------------------------------------------ Keys

const KEY_PA: i32 = 400;
const KEY_PQ: i32 = 401;
const KEY_CA: i32 = 402;
const KEY_DS: i32 = 403;
const KEY_HC: i32 = 404;
const KEY_DA: i32 = 405;
const KEY_IM: i32 = 406;
const KEY_MS: i32 = 407;
const KEY_MF: i32 = 408;
const KEY_PF: i32 = 409;
const KEY_AP: i32 = 410;
const KEY_OBS: i32 = 411;
const KEY_GI_DA: i32 = 412;
const KEY_MCV_DA: i32 = 413;
const KEY_ORDER_RESULT: i32 = 420;
const KEY_COMMANDED: i32 = 421;
const KEY_DEPLOYED: i32 = 422;
const KEY_GI_DEPLOYED: i32 = 423;
const KEY_MCV_DEPLOYED: i32 = 424;
const KEY_BIRTH_YARD: i32 = 425;
const KEY_BIRTH_SURVIVOR: i32 = 426;

/// `RefuseReason` `Vetoed` (appendix 8.3, enum 26).
const VETOED: i64 = 9;
/// A finished state machine.
const DONE: i64 = 99;

// ----------------------------------------------------------------- State

struct V(AtomicI64);

impl V {
    const fn new(value: i64) -> Self {
        Self(AtomicI64::new(value))
    }

    fn get(&self) -> i64 {
        self.0.load(Relaxed)
    }

    fn set(&self, value: i64) {
        self.0.store(value, Relaxed)
    }

    fn add(&self, value: i64) {
        self.0.fetch_add(value, Relaxed);
    }

    fn unit(&self) -> Techno {
        Techno(self.get())
    }

    fn cell(&self) -> Cell {
        Cell(self.get())
    }
}

static OPEN_AT: V = V::new(0);

// MoveFrame, then Push and SetFacing, on one tank.
static T_MF: V = V::new(0);
static TR_MF: V = V::new(0);
static MF: V = V::new(0);
static MF_X: V = V::new(0);
static MF_Y: V = V::new(0);
static MF_TRIES: V = V::new(0);
static PU: V = V::new(0);
static PU_AT: V = V::new(0);
static PU_X: V = V::new(0);
static PU_Y: V = V::new(0);
static PU_DX: V = V::new(0);
static PU_DY: V = V::new(0);
static SF: V = V::new(0);
static SF_AT: V = V::new(0);
static SF_TO: V = V::new(0);

// PathFound.
static T_PF: V = V::new(0);
static PF: V = V::new(0);
static PF_AT: V = V::new(0);
static PF_START: V = V::new(0);
static PF_GOAL: V = V::new(0);
static PF_SIDE: V = V::new(1);
static PF_TRIES: V = V::new(0);

// IdleMission.
static T_IM: V = V::new(0);
static IM: V = V::new(0);
static IM_AT: V = V::new(0);
static IM_GOAL: V = V::new(0);
static IM_EXPECT: V = V::new(0);
static IM_HOME: V = V::new(0);
static IM_TRY: V = V::new(0);

// MissionStep.
static T_MS: V = V::new(0);
static MS: V = V::new(0);
static MS_AT: V = V::new(0);
static MS_DEST: V = V::new(0);

// ApproachPoint, the engine test's shape: a player tank attacks a computer GI (weapons
// off, asleep) seven cells away, inside its sight and out of its range.
static T_AP2: V = V::new(0);
static AP2_TARGET: V = V::new(0);
static AP2: V = V::new(0);
static AP2_AT: V = V::new(0);
static AP2_CELL: V = V::new(0);
static AP2_SEEN: V = V::new(0);

// The MoveFrame observer (never answers).
static TR_OBS: V = V::new(0);

// CommandAllowed, then DestinationSet, on one computer-house miner.
static U_CMD: V = V::new(0);
static CA: V = V::new(0);
static CA_AT: V = V::new(0);
static CA_VETO: V = V::new(0);
static CA_ALLOW: V = V::new(0);
static CA_TRIES: V = V::new(0);
static DS: V = V::new(0);
static DS_AT: V = V::new(0);
static DS_SEEN: V = V::new(0);
static DS_NATIVE: V = V::new(0);
static CMD_NATIVE: V = V::new(0);
static CMD_MODULE: V = V::new(0);

// HarvestCell, on the computer house's own harvesters.
static HC: V = V::new(0);
static HC_AT: V = V::new(0);
static HC_UNIT: V = V::new(0);
static HC_CELL: V = V::new(0);
static HC_SEEN: V = V::new(0);
static HC_OTHER: V = V::new(0);
static HC_BOUND: V = V::new(0);
static HC_TRIES: V = V::new(0);

// DeployAllowed on a Siege Chopper.
static U_DEPLOY: V = V::new(0);
static TR_DA: V = V::new(0);
static TR_DEPLOYED: V = V::new(0);
static DA: V = V::new(0);
static DA_AT: V = V::new(0);
static DA_KIND: V = V::new(0);
static DA_ALLOW: V = V::new(0);
static DA_BECAME: V = V::new(0);

// DeployInfantry: a standing computer GI.
static U_GI: V = V::new(0);
static TR_GI_DA: V = V::new(0);
static TR_GI_DEPLOYED: V = V::new(0);
static GI: V = V::new(0);
static GI_AT: V = V::new(0);
static GI_ASKED: V = V::new(0);
static GI_ALLOW: V = V::new(0);
static GI_DEPLOYED: V = V::new(0);

// DeployMcv and BirthYard: a computer MCV in the computer's base.
static U_MCV: V = V::new(0);
static TR_MCV_DA: V = V::new(0);
static TR_MCV_DEPLOYED: V = V::new(0);
static TR_BIRTH_YARD: V = V::new(0);
static MC: V = V::new(0);
static MC_AT: V = V::new(0);
static MC_AFTER: V = V::new(0);
static MC_BUILDING: V = V::new(0);
static MC_RUNS: V = V::new(0);
static MC_TRIES: V = V::new(0);
static MC_ASKED: V = V::new(0);
static MC_ALLOW: V = V::new(0);
static MC_YARD: V = V::new(0);
static MC_BORN: V = V::new(0);
static MC_CLAIM: V = V::new(0);

// BirthSurvivor: a destroyed player power plant.
static U_PLANT: V = V::new(0);
static TR_BIRTH_SURVIVOR: V = V::new(0);
static SV: V = V::new(0);
static SV_AT: V = V::new(0);
static SV_TRIES: V = V::new(0);
static SV_SEEN: V = V::new(0);
static SV_WARHEAD: V = V::new(0);

// ProductionQueued, then ProductionAllowed, on the computer house's own
// production.
static TR_PA: V = V::new(0);
static TR_PQ: V = V::new(0);
static TR_PQ2: V = V::new(0);
static PROD_TYPE: V = V::new(-1);
static PQ: V = V::new(0);
static PQ_AT: V = V::new(0);
static PQ_NO: V = V::new(0);
static PQ_YES: V = V::new(0);
static PQ_COUNT: V = V::new(0);
static PA: V = V::new(0);
static PA_AT: V = V::new(0);
static PA_NO: V = V::new(0);
static PA_NATIVE: V = V::new(0);
static PA_YES: V = V::new(0);
static PA_TYPE: V = V::new(-1);
static PA_CANDIDATE: V = V::new(0);
static PA_CLAIM: V = V::new(0);
/// The latest `OrderRefused` reason for the queued type, plus one (0 none).
static PA_REFUSED: V = V::new(0);

// --------------------------------------------------------------- Helpers

fn cell_of(unit: Techno) -> Cell {
    Cell(get(unit, Prop::Cell).unwrap_or(NO_HANDLE))
}

/// Chebyshev distance in cells.
fn dist(a: Cell, b: Cell) -> i32 {
    let dx = (i32::from(a.x()) - i32::from(b.x())).abs();
    let dy = (i32::from(a.y()) - i32::from(b.y())).abs();
    dx.max(dy)
}

fn passable(cell: Cell) -> bool {
    cell.passable(SpeedType::Track).unwrap_or(false)
}

/// The first of `offsets` from `from` a tracked unit may drive on, else the
/// first.
fn pick(from: Cell, offsets: &[(i32, i32)]) -> Cell {
    offsets
        .iter()
        .map(|&(dx, dy)| offset(from, dx, dy))
        .find(|&cell| passable(cell))
        .unwrap_or_else(|| offset(from, offsets[0].0, offsets[0].1))
}

/// Spawns `id` for `house` near `at` and keeps it in `slot`.
fn put(slot: &V, at: Cell, id: &str, house: House) -> Option<Techno> {
    let unit = spawn(at, id, house)?;
    slot.set(unit.0);
    Some(unit)
}

/// A decision trigger on `kind` bound to `to`; its handle, 0 when it failed.
fn decider(kind: i32, to: impl Ref, key: i32) -> i64 {
    Trigger::on(kind)
        .bind(to)
        .decide_sync(key)
        .map_or(0, |trigger| trigger.0)
}

fn disable(slot: &V) {
    if slot.get() != 0 {
        let _ = TriggerRef(slot.get()).set_enabled(false);
        slot.set(0);
    }
}

fn no_cell() -> Cell {
    Cell(NO_HANDLE)
}

/// The current frame (this file's `frame` is the area hook).
fn tick() -> i64 {
    i64::from(vera20k_sdk::frame())
}

// ------------------------------------------------------------------ Hooks

/// `vera_init`.
pub fn init() {}

/// `on_scenario_start`.
pub fn start() {}

/// The frame the arena opens: the houses and `crate::arena` are known.
pub fn open() {
    OPEN_AT.set(tick());
    let a = arena(crate::MOVEMENT);
    let me = player();
    let them = enemy();

    // The player's tanks, west half of the arena.
    if let Some(tank) = put(&T_MF, offset(a, -6, -6), "MTNK", me) {
        TR_MF.set(decider(Event::MOVE_FRAME, tank, KEY_MF));
    }
    if let Some(tank) = put(&T_PF, offset(a, -2, -6), "MTNK", me) {
        let at = cell_of(tank);
        PF_START.set(at.0);
        PF_GOAL.set(offset(at, 0, 4).0);
        let side = [1, -1]
            .into_iter()
            .find(|&s| (1..=3).all(|dy| passable(offset(at, s, dy))))
            .unwrap_or(1);
        PF_SIDE.set(i64::from(side));
        decider(Event::PATH_FOUND, tank, KEY_PF);
    }
    if let Some(tank) = put(&T_IM, offset(a, -6, -1), "MTNK", me) {
        IM_GOAL.set(pick(cell_of(tank), &[(0, 3), (3, 0), (0, -3), (-3, 0)]).0);
        decider(Event::IDLE_MISSION, tank, KEY_IM);
    }
    if let Some(tank) = put(&T_MS, offset(a, -6, 4), "MTNK", me) {
        MS_DEST.set(pick(cell_of(tank), &[(3, -2), (3, 2), (-3, 2), (-3, -2)]).0);
        decider(Event::MISSION_STEP, tank, KEY_MS);
    }
    if let Some(tank) = put(&T_AP2, offset(a, 3, 7), "MTNK", me) {
        let at = cell_of(tank);
        if let Some(gi) = put(&AP2_TARGET, offset(at, 0, 7), "E1", them) {
            let _ = Modifier::override_(gi, Prop::DisableWeapons, 1, -1);
            let _ = Action::set_mission(gi, Mission::Sleep, no_cell());
            AP2_CELL.set(pick(at, &[(0, -2), (2, -2), (-2, -2), (2, 0), (-2, 0)]).0);
            decider(Event::APPROACH_POINT, tank, KEY_AP);
            observe(tank);
        }
    }

    // The computer house's objects, east half (unarmed or weapons off).
    if let Some(miner) = put(&U_CMD, offset(a, 7, -6), "CMIN", them) {
        let at = cell_of(miner);
        CMD_NATIVE.set(pick(at, &[(3, 0), (-3, 0)]).0);
        CMD_MODULE.set(pick(at, &[(0, 4), (0, -4)]).0);
        decider(Event::COMMAND_ALLOWED, miner, KEY_CA);
        decider(Event::DESTINATION_SET, miner, KEY_DS);
        observe(miner);
        let _ = Trigger::on(Event::COMMANDED)
            .bind(miner)
            .enqueue(KEY_COMMANDED)
            .done();
    }
    let _ = Trigger::on(Event::ORDER_REFUSED)
        .enqueue(KEY_ORDER_RESULT)
        .done();

    // The computer house's own production and harvesters.
    production_open();
    type_decider(Event::HARVEST_CELL, &["HARV", "CMIN", "SMIN"], KEY_HC);
}

/// Every later frame.
pub fn frame(now: i32) {
    let now = i64::from(now);
    let since = now - OPEN_AT.get();
    move_frame(now, since);
    push_and_facing(now);
    path_found(now, since);
    idle_mission(now, since);
    mission_step(now, since);
    approach_point_player(now, since);
    command_allowed(now, since);
    destination_set(now);
    harvest_cell(now, since);
    deploy_allowed(now, since);
    deploy_infantry(now, since);
    deploy_mcv(now, since);
    birth_survivor(now, since);
    production(now, since);
}

/// Records whose key is in this area's range.
pub fn events(record: &EventRecord) {
    match record.key {
        KEY_ORDER_RESULT => order_refused(record),
        KEY_COMMANDED => {
            if record.subject == U_CMD.get()
                && record.value == Command::Guard as i64
                && CA.get() == 3
                && CA_ALLOW.get() == 1
            {
                mark("movement.CommandAllowed");
                CA.set(DONE);
            }
        }
        KEY_DEPLOYED => {
            if record.subject != U_DEPLOY.get() || record.value != 1 {
                return;
            }
            match DA.get() {
                2 => {
                    bench_fail("movement.DeployAllowed", "deployed after the veto");
                    DA.set(DONE);
                }
                3 if DA_ALLOW.get() == 1 => {
                    mark("movement.DeployAllowed");
                    DA_BECAME.set(record.other);
                    DA.set(4);
                }
                _ => {}
            }
        }
        KEY_GI_DEPLOYED => gi_deployed(record),
        KEY_MCV_DEPLOYED => mcv_deployed(record),
        KEY_BIRTH_YARD => yard_born(record),
        KEY_BIRTH_SURVIVOR => survivor_born(record),
        _ => {}
    }
}

/// Decisions whose key is in this area's range.
pub fn decide(record: &mut DecideRecord) {
    let key = record.event.key;
    match key {
        KEY_PA => decide_production_allowed(ProductionAllowedDecision::from(record)),
        KEY_PQ => decide_production_queued(ProductionQueuedDecision::from(record)),
        KEY_CA => decide_command(CommandAllowedDecision::from(record)),
        KEY_DS => decide_destination(DestinationSetDecision::from(record)),
        KEY_HC => decide_harvest(HarvestCellDecision::from(record)),
        KEY_DA => decide_deploy(DeployAllowedDecision::from(record)),
        KEY_GI_DA => decide_gi_deploy(DeployAllowedDecision::from(record)),
        KEY_MCV_DA => decide_mcv_deploy(DeployAllowedDecision::from(record)),
        KEY_IM => decide_idle(IdleMissionDecision::from(record)),
        KEY_MS => decide_step(MissionStepDecision::from(record)),
        KEY_MF => decide_move_frame(MoveFrameDecision::from(record)),
        KEY_PF => decide_path(PathFoundDecision::from(record)),
        KEY_AP => decide_approach(ApproachPointDecision::from(record)),
        KEY_OBS => decide_observe(MoveFrameDecision::from(record)),
        _ => {}
    }
}

// ------------------------------------------- MoveFrame, Push, SetFacing

fn move_frame(_now: i64, since: i64) {
    match MF.get() {
        0 if since >= 10 && alive(T_MF.unit()) => MF.set(1),
        1 | 2 if since > 600 => MF.set(DONE),
        DONE => disable(&TR_MF),
        _ => {}
    }
}

/// A 16-lepton step of the resting tank; the next request's position must
/// be the old one plus the step (one retry the other way).
fn decide_move_frame(mut d: MoveFrameDecision) {
    if d.unit().0 != T_MF.get() {
        return;
    }
    let [x, y, _] = d.position();
    let (x, y) = (i64::from(x), i64::from(y));
    match MF.get() {
        1 => {
            if d.moving() {
                return;
            }
            let dx = if MF_TRIES.get() == 0 { 16 } else { -16 };
            if d.step_by(dx as i32, 0) {
                MF_X.set(x + dx);
                MF_Y.set(y);
                MF.set(2);
            }
        }
        2 => {
            if x == MF_X.get() && y == MF_Y.get() {
                mark("movement.MoveFrame");
                MF.set(DONE);
            } else if MF_TRIES.get() == 0 {
                MF_TRIES.set(1);
                MF.set(1);
            } else {
                bench_fail("movement.MoveFrame", "the step did not land");
                MF.set(DONE);
            }
        }
        _ => {}
    }
}

fn push_and_facing(now: i64) {
    let tank = T_MF.unit();
    match PU.get() {
        0 if MF.get() == DONE => {
            if !alive(tank) {
                PU.set(DONE);
                return;
            }
            let at = cell_of(tank);
            let dir = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .into_iter()
                .find(|&(dx, dy)| (1..=3).all(|k| passable(offset(at, dx * k, dy * k))))
                .unwrap_or((1, 0));
            let (Ok(x), Ok(y)) = (get(tank, Prop::X), get(tank, Prop::Y)) else {
                PU.set(DONE);
                return;
            };
            if Action::push(tank, offset(at, dir.0 * 3, dir.1 * 3), 128, 4).is_ok() {
                PU_X.set(x);
                PU_Y.set(y);
                PU_DX.set(i64::from(dir.0));
                PU_DY.set(i64::from(dir.1));
                PU_AT.set(now);
                PU.set(1);
            } else {
                bench_fail("movement.Push", "Push answered an error");
                PU.set(DONE);
            }
        }
        1 if now >= PU_AT.get() + 12 => {
            let (Ok(x), Ok(y)) = (get(tank, Prop::X), get(tank, Prop::Y)) else {
                PU.set(DONE);
                return;
            };
            let moved = (x - PU_X.get()) * PU_DX.get() + (y - PU_Y.get()) * PU_DY.get();
            if moved >= 96 {
                mark("movement.Push");
            } else {
                bench_fail("movement.Push", "the tank did not move");
            }
            PU.set(DONE);
        }
        _ => {}
    }
    match SF.get() {
        0 if PU.get() == DONE => {
            let Ok(facing) = tank.facing() else {
                SF.set(DONE);
                return;
            };
            let to = (facing + 64) & 255;
            if Action::set_facing(tank, to as u8).is_ok() {
                SF_TO.set(to);
                SF_AT.set(now);
                SF.set(1);
            } else {
                bench_fail("movement.SetFacing", "SetFacing answered an error");
                SF.set(DONE);
            }
        }
        1 => {
            let off = tank
                .facing()
                .map_or(128, |f| (f - SF_TO.get()).rem_euclid(256));
            if off.min(256 - off) <= 3 {
                mark("movement.SetFacing");
                SF.set(DONE);
            } else if now > SF_AT.get() + 120 {
                bench_fail("movement.SetFacing", "the body did not turn");
                SF.set(DONE);
            }
        }
        _ => {}
    }
}

// ------------------------------------------------------------- PathFound

/// Orders the tank four cells south; the first search answers no route
/// (it must stay in its cell), the next one a detour a column to the side
/// (it must drive through it).
fn path_found(now: i64, since: i64) {
    let tank = T_PF.unit();
    if PF.get() != DONE && PF.get() != 0 && !alive(tank) {
        PF.set(DONE);
        return;
    }
    match PF.get() {
        0 if since >= 10 && alive(tank) => {
            if Action::set_mission(tank, Mission::Move, PF_GOAL.cell()).is_ok() {
                PF_AT.set(now);
                PF.set(1);
            } else {
                PF.set(DONE);
            }
        }
        1 | 3 if now > PF_AT.get() + 200 => {
            PF_TRIES.add(1);
            if PF_TRIES.get() > 2
                || Action::set_mission(tank, Mission::Move, PF_GOAL.cell()).is_err()
            {
                PF.set(DONE);
            }
            PF_AT.set(now);
        }
        2 if now >= PF_AT.get() + 25 => {
            if cell_of(tank) != PF_START.cell() {
                bench_fail(
                    "movement.PathFound",
                    "the tank left its cell without a route",
                );
                PF.set(DONE);
            } else if Action::set_mission(tank, Mission::Move, PF_GOAL.cell()).is_ok() {
                PF_AT.set(now);
                PF.set(3);
            } else {
                PF.set(DONE);
            }
        }
        4 => {
            let (at, start) = (cell_of(tank), PF_START.cell());
            let side = i32::from(at.x()) - i32::from(start.x());
            let down = i32::from(at.y()) - i32::from(start.y());
            if i64::from(side) == PF_SIDE.get() && (1..=3).contains(&down) {
                mark("movement.PathFound");
                PF.set(DONE);
            } else if now > PF_AT.get() + 300 {
                bench_fail(
                    "movement.PathFound",
                    "the tank did not drive the module route",
                );
                PF.set(DONE);
            }
        }
        _ => {}
    }
}

fn decide_path(mut d: PathFoundDecision) {
    if d.unit().0 != T_PF.get() {
        return;
    }
    match PF.get() {
        1 => {
            d.no_route();
            PF_AT.set(tick());
            PF.set(2);
        }
        2 => d.no_route(),
        3 => {
            let [x, y, _] = d.position();
            let (cx, cy) = (x.div_euclid(256), y.div_euclid(256));
            let start = PF_START.cell();
            if (cx, cy) != (i32::from(start.x()), i32::from(start.y())) {
                return;
            }
            let s = PF_SIDE.get() as i32;
            let route = [
                sys::PathCell {
                    x: cx + s,
                    y: cy + 1,
                },
                sys::PathCell {
                    x: cx + s,
                    y: cy + 2,
                },
                sys::PathCell {
                    x: cx + s,
                    y: cy + 3,
                },
                sys::PathCell { x: cx, y: cy + 4 },
            ];
            if d.set_cells(&route) {
                PF_AT.set(tick());
                PF.set(4);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------- IdleMission

/// Moves the tank three cells; every idle request after the order is
/// answered (attempt 0: Guard and Area Guard swapped; attempt 1, back home:
/// Sleep), and the answer must become its mission.
fn idle_mission(now: i64, since: i64) {
    let tank = T_IM.unit();
    match IM.get() {
        0 if since >= 12 && alive(tank) => {
            IM_HOME.set(cell_of(tank).0);
            IM.set(1);
            IM_AT.set(now);
            if Action::set_mission(tank, Mission::Move, IM_GOAL.cell()).is_err() {
                IM.set(DONE);
            }
        }
        1 if now > IM_AT.get() + 400 => {
            IM.set(DONE);
        }
        2 => {
            let mission = tank.mission().unwrap_or(-1);
            if mission == IM_EXPECT.get() {
                mark("movement.IdleMission");
                IM.set(DONE);
            } else if now > IM_AT.get() + 300 {
                if IM_TRY.get() == 0 {
                    IM_TRY.set(1);
                    IM_AT.set(now);
                    IM.set(1);
                    if Action::set_mission(tank, Mission::Move, IM_HOME.cell()).is_err() {
                        IM.set(DONE);
                    }
                } else {
                    bench_fail("movement.IdleMission", "the unit kept another mission");
                    IM.set(DONE);
                }
            }
        }
        _ => {}
    }
}

fn decide_idle(mut d: IdleMissionDecision) {
    if d.unit().0 != T_IM.get() || !matches!(IM.get(), 1 | 2) {
        return;
    }
    if IM.get() == 1 {
        let native = d.native();
        let answer = if IM_TRY.get() == 1 {
            Mission::Sleep as i64
        } else if native == Mission::AreaGuard as i64 {
            Mission::Guard as i64
        } else {
            Mission::AreaGuard as i64
        };
        IM_EXPECT.set(answer);
        IM_AT.set(tick());
        IM.set(2);
    }
    d.set_result(IM_EXPECT.get());
}

// ---------------------------------------------------------- MissionStep

/// A guarding tank: the first Guard-family body's outcome gets the
/// module's destination, where the tank must arrive.
fn mission_step(now: i64, since: i64) {
    let tank = T_MS.unit();
    match MS.get() {
        0 if since >= 12 && alive(tank) => {
            MS.set(1);
            MS_AT.set(now);
            if Action::set_mission(tank, Mission::Guard, no_cell()).is_err() {
                MS.set(DONE);
            }
        }
        1 if now > MS_AT.get() + 300 => MS.set(DONE),
        2 => {
            if dist(cell_of(tank), MS_DEST.cell()) <= 1 {
                mark("movement.MissionStep");
                MS.set(DONE);
            } else if now > MS_AT.get() + 300 {
                bench_fail(
                    "movement.MissionStep",
                    "the tank did not reach the module cell",
                );
                MS.set(DONE);
            }
        }
        _ => {}
    }
}

fn decide_step(mut d: MissionStepDecision) {
    if d.unit().0 != T_MS.get() || MS.get() != 1 {
        return;
    }
    if !matches!(d.mission(), 1 | 5 | 6 | 11 | 15) {
        return;
    }
    if d.step().is_none() {
        bench_fail("movement.MissionStep", "no result area");
        MS.set(DONE);
        return;
    }
    if d.set_destination(MS_DEST.cell()) {
        MS_AT.set(tick());
        MS.set(2);
    }
}

// -------------------------------------------------------- ApproachPoint

/// A player tank ordered (`SetMission` Attack) onto a sleeping computer
/// GI (weapons off) seven cells away, in sight and out of range, as the
/// engine test does: every approach answers a cell two cells north, which
/// must become its NavCom (seen by the observer) or where it arrives.
fn approach_point_player(now: i64, since: i64) {
    let tank = T_AP2.unit();
    let gi = AP2_TARGET.unit();
    match AP2.get() {
        0 if since >= 15 => {
            if !alive(tank) || !alive(gi) {
                AP2.set(DONE);
            } else if Action::set_mission_on(tank, Mission::Attack, gi).is_ok() {
                AP2_AT.set(now);
                AP2.set(1);
            } else {
                ap2_finish();
            }
        }
        1 if now > AP2_AT.get() + 300 => {
            ap2_finish();
        }
        2 => {
            if AP2_SEEN.get() == 1 || dist(cell_of(tank), AP2_CELL.cell()) <= 1 {
                mark("movement.ApproachPoint");
                ap2_finish();
            } else if now > AP2_AT.get() + 300 {
                bench_fail(
                    "movement.ApproachPoint",
                    "the tank did not head for the module cell",
                );
                ap2_finish();
            }
        }
        _ => {}
    }
}

fn ap2_finish() {
    let tank = T_AP2.unit();
    if alive(tank) {
        let _ = Action::set_mission(tank, Mission::Guard, no_cell());
    }
    let gi = AP2_TARGET.unit();
    if alive(gi) {
        let _ = Action::remove(gi);
    }
    AP2.set(DONE);
}

fn decide_approach(mut d: ApproachPointDecision) {
    if d.unit().0 == T_AP2.get() && matches!(AP2.get(), 1 | 2) {
        if d.target().0 != AP2_TARGET.get() || d.chosen().is_none() {
            bench_fail("movement.ApproachPoint", "the request named another target");
        }
        d.set_cell(AP2_CELL.cell());
        if AP2.get() == 1 {
            AP2_AT.set(tick());
            AP2.set(2);
        }
    }
}

// ------------------------------------------------- the MoveFrame observer

/// Binds the observer (a `MoveFrame` decider that never answers) to
/// `unit`: its requests carry the unit's NavCom, the effect of a
/// `DestinationSet`, `ApproachPoint` or `HarvestCell` answer.
fn observe(unit: Techno) {
    if TR_OBS.get() == 0 {
        TR_OBS.set(decider(Event::MOVE_FRAME, unit, KEY_OBS));
    } else {
        let _ = TriggerRef(TR_OBS.get()).bind(unit);
    }
}

fn decide_observe(d: MoveFrameDecision) {
    let unit = d.unit().0;
    let navcom = d.destination_cell();
    if unit == U_CMD.get() && DS.get() == 2 {
        if navcom == CMD_MODULE.cell() {
            DS_SEEN.set(1);
        } else if navcom == CMD_NATIVE.cell() {
            DS_NATIVE.set(1);
        }
    }
    if unit == T_AP2.get() && AP2.get() == 2 && navcom == AP2_CELL.cell() {
        AP2_SEEN.set(1);
    }
    if unit == HC_UNIT.get() && HC.get() == 2 {
        if navcom == HC_CELL.cell() {
            HC_SEEN.set(1);
        } else if !navcom.is_none() {
            HC_OTHER.set(1);
        }
    }
}

// ---------------------------------------- CommandAllowed, DestinationSet

/// A Move order is vetoed (its `OrderRefused` must say `Vetoed`), then a
/// Guard order is allowed (its `Commanded` must come).
fn command_allowed(now: i64, since: i64) {
    let miner = U_CMD.unit();
    let units = [miner];
    match CA.get() {
        0 if since >= 5 && alive(miner) => {
            CA.set(1);
            CA_AT.set(now);
            if enemy()
                .order(&Order::move_to(&units, CMD_NATIVE.cell()))
                .is_err()
            {
                CA.set(DONE);
            }
        }
        1 if now > CA_AT.get() + 60 => {
            CA_TRIES.add(1);
            CA_VETO.set(0);
            CA_AT.set(now);
            if CA_TRIES.get() > 3
                || !alive(miner)
                || enemy()
                    .order(&Order::move_to(&units, CMD_NATIVE.cell()))
                    .is_err()
            {
                CA.set(DONE);
            }
        }
        2 => {
            CA.set(3);
            CA_AT.set(now);
            if enemy()
                .order(&Order::guard(&units, Techno(NO_HANDLE)))
                .is_err()
            {
                CA.set(DONE);
            }
        }
        3 if now > CA_AT.get() + 60 => CA.set(DONE),
        _ => {}
    }
}

fn decide_command(mut d: CommandAllowedDecision) {
    if d.unit().0 != U_CMD.get() {
        return;
    }
    let command = d.command();
    match CA.get() {
        1 if command == Command::Move as i32 => {
            d.set_result(0);
            CA_VETO.set(1);
        }
        3 if command == Command::Guard as i32 => {
            d.set_result(1);
            CA_ALLOW.set(1);
        }
        _ => {}
    }
}
/// After the commands: a Move order to one cell (three cells from where
/// the miner now stands) whose destination the module replaces with
/// another; the observer must see that cell as its NavCom (or it arrives).
fn destination_set(now: i64) {
    let miner = U_CMD.unit();
    match DS.get() {
        0 if CA.get() == DONE => {
            if !alive(miner) {
                DS.set(DONE);
                return;
            }
            // The miner may have driven off to harvest meanwhile.
            let at = cell_of(miner);
            CMD_NATIVE.set(pick(at, &[(3, 0), (-3, 0)]).0);
            CMD_MODULE.set(pick(at, &[(0, 4), (0, -4)]).0);
            let units = [miner];
            if enemy()
                .order(&Order::move_to(&units, CMD_NATIVE.cell()))
                .is_err()
            {
                DS.set(DONE);
                return;
            }
            DS_AT.set(now);
            DS.set(1);
        }
        1 if now > DS_AT.get() + 200 => {
            DS.set(DONE);
        }
        2 => {
            if DS_SEEN.get() == 1 || dist(cell_of(miner), CMD_MODULE.cell()) <= 1 {
                mark("movement.DestinationSet");
                DS.set(DONE);
            } else if now > DS_AT.get() + 150 || !alive(miner) {
                if DS_NATIVE.get() == 1 {
                    bench_fail("movement.DestinationSet", "the native destination stayed");
                }
                DS.set(DONE);
            }
        }
        _ => {}
    }
}

fn decide_destination(mut d: DestinationSetDecision) {
    if d.object().0 != U_CMD.get() || !matches!(DS.get(), 1 | 2) {
        return;
    }
    if d.destination_cell() == CMD_NATIVE.cell() {
        d.set_result(CMD_MODULE.get());
        if DS.get() == 1 {
            DS_AT.set(tick());
            DS.set(2);
        }
    }
}

// ----------------------------------------------------------- HarvestCell

/// The computer house's own harvesters (a miner made by `create_object`
/// never scanned in the first runs): one scan is answered with another ore
/// cell of the field, which the observer must see as the harvester's
/// NavCom (or it arrives). Up to [`HC_MAX_TRIES`] tries.
/// Answered scans tried before giving up. The scans come when a computer
/// harvester looks for ore (the first one after ~1900 frames in the bench's
/// scene), so the area waits for them for the whole run.
const HC_MAX_TRIES: i64 = 12;

fn harvest_cell(now: i64, since: i64) {
    match HC.get() {
        0 if since >= 3 => {
            HC.set(1);
            HC_AT.set(now);
        }
        2 => {
            let harvester = HC_UNIT.unit();
            if HC_BOUND.get() != harvester.0 {
                observe(harvester);
                HC_BOUND.set(harvester.0);
            }
            if HC_SEEN.get() == 1 || dist(cell_of(harvester), HC_CELL.cell()) <= 1 {
                mark("movement.HarvestCell");
                HC.set(DONE);
            } else if now > HC_AT.get() + 300 || !alive(harvester) {
                HC_TRIES.add(1);
                if HC_TRIES.get() < HC_MAX_TRIES {
                    HC_AT.set(now);
                    HC.set(1);
                } else {
                    if HC_OTHER.get() == 1 {
                        bench_fail(
                            "movement.HarvestCell",
                            "the harvester headed for another cell",
                        );
                    }
                    HC.set(DONE);
                }
            }
        }
        _ => {}
    }
}

fn decide_harvest(mut d: HarvestCellDecision) {
    if HC.get() != 1 {
        return;
    }
    let harvester = d.harvester();
    if get(harvester, Prop::Owner).ok() != Some(enemy().0) {
        return;
    }
    let scanned = d.scanned();
    if scanned.is_none() {
        return;
    }
    let mut cells = [NO_HANDLE; 32];
    let count = Find::cells()
        .near(scanned, 5 * 256)
        .where_prop(Prop::Ore, Op::Gt, 0)
        .page(&mut cells, 0)
        .map_or(0, |(count, _)| count);
    let Some(far) = cells[..count]
        .iter()
        .copied()
        .filter(|&c| dist(Cell(c), scanned) >= 2)
        .max_by_key(|&c| dist(Cell(c), scanned))
    else {
        return;
    };
    d.set_result(far);
    HC_UNIT.set(harvester.0);
    HC_CELL.set(far);
    HC_SEEN.set(0);
    HC_OTHER.set(0);
    HC_AT.set(tick());
    HC.set(2);
}

// --------------------------------------------------------- DeployAllowed

/// The deployers tried in turn: a Siege Chopper (a simple deployer, asked
/// in its Unload mission). An MCV has its own scene in the computer's base
/// (`deploy_mcv`): a computer yard deployed next to the player's start
/// would move the computer's base there.
const DEPLOYERS: [&str; 1] = ["SCHP"];

/// A cell whose 5x5 block around it is level and passable, near `center`.
fn flat_spot(center: Cell) -> Cell {
    for ring in 0..=3i32 {
        for dy in -ring..=ring {
            for dx in -ring..=ring {
                if dx.abs().max(dy.abs()) != ring {
                    continue;
                }
                let spot = offset(center, dx, dy);
                let Ok(level) = spot.height() else {
                    continue;
                };
                let flat = (-2..=2).all(|y| {
                    (-2..=2).all(|x| {
                        let cell = offset(spot, x, y);
                        cell.height().is_ok_and(|h| h == level) && passable(cell)
                    })
                });
                if flat {
                    return spot;
                }
            }
        }
    }
    center
}

/// Spawns deployer `DA_KIND` for the computer house and binds the deploy
/// triggers to it; false when none is left or it cannot be placed.
fn next_deployer() -> bool {
    while let Some(id) = DEPLOYERS.get(DA_KIND.get() as usize) {
        let spot = flat_spot(offset(arena(crate::MOVEMENT), 7, 4));
        if let Some(unit) = put(&U_DEPLOY, spot, id, enemy()) {
            let _ = Modifier::override_(unit, Prop::DisableWeapons, 1, -1);
            if TR_DA.get() == 0 {
                TR_DA.set(decider(Event::DEPLOY_ALLOWED, unit, KEY_DA));
                TR_DEPLOYED.set(
                    Trigger::on(Event::DEPLOYED)
                        .bind(unit)
                        .enqueue(KEY_DEPLOYED)
                        .done()
                        .map_or(0, |trigger| trigger.0),
                );
            } else {
                let _ = TriggerRef(TR_DA.get()).bind(unit);
                let _ = TriggerRef(TR_DEPLOYED.get()).bind(unit);
            }
            return true;
        }
        DA_KIND.add(1);
    }
    false
}

fn order_deploy(unit: Techno) -> bool {
    let units = [unit];
    enemy().order(&Order::deploy_mcv(&units)).is_ok()
}

/// The deploy is vetoed (no `Deployed` may follow), then allowed (the
/// `Deployed` must come); a deployer never asked gives way to the next.
fn deploy_allowed(now: i64, since: i64) {
    let unit = U_DEPLOY.unit();
    match DA.get() {
        0 if since >= 20 => {
            if !next_deployer() {
                DA.set(DONE);
                return;
            }
            DA.set(5);
            DA_AT.set(now);
        }
        // Let it settle (a chopper lands) before the order.
        5 if now >= DA_AT.get() + 15 => {
            DA.set(1);
            DA_AT.set(now);
            let _ = order_deploy(unit);
        }
        1 if now > DA_AT.get() + 150 => {
            if alive(unit) {
                let _ = Action::remove(unit);
            }
            DA_KIND.add(1);
            DA.set(0);
        }
        2 if now >= DA_AT.get() + 30 => {
            DA.set(3);
            DA_AT.set(now);
            if !alive(unit) || !order_deploy(unit) {
                DA.set(DONE);
            }
        }
        3 if now > DA_AT.get() + 200 => {
            DA.set(4);
        }
        4 => {
            // Leave nothing of the computer's next to the player's start.
            let became = Techno(DA_BECAME.get());
            if !became.is_none() && alive(became) {
                let _ = Action::remove(became);
            }
            if alive(unit) {
                let _ = Action::remove(unit);
            }
            DA.set(DONE);
        }
        _ => {}
    }
}

fn decide_deploy(mut d: DeployAllowedDecision) {
    if d.object().0 != U_DEPLOY.get() {
        return;
    }
    match DA.get() {
        1 => {
            d.set_result(0);
            DA_AT.set(tick());
            DA.set(2);
        }
        3 => {
            d.set_result(1);
            DA_ALLOW.set(1);
        }
        _ => {}
    }
}

// -------------------------------------------------------- DeployInfantry

/// A standing computer GI (weapons off, far from the player's tanks) is
/// ordered to deploy (`ToggleInfantryDeploy`, its Unload mission): the
/// deploy is refused (it must be asked, and the GI must stay standing),
/// then allowed (its `Deployed` must come and `Deployed` read 1).
fn deploy_infantry(now: i64, since: i64) {
    let gi = U_GI.unit();
    match GI.get() {
        0 if since >= 30 => {
            let Some(gi) = put(&U_GI, offset(arena(crate::MOVEMENT), 10, -1), "E1", enemy()) else {
                GI.set(DONE);
                return;
            };
            let _ = Modifier::override_(gi, Prop::DisableWeapons, 1, -1);
            TR_GI_DA.set(decider(Event::DEPLOY_ALLOWED, gi, KEY_GI_DA));
            TR_GI_DEPLOYED.set(
                Trigger::on(Event::DEPLOYED)
                    .bind(gi)
                    .enqueue(KEY_GI_DEPLOYED)
                    .done()
                    .map_or(0, |trigger| trigger.0),
            );
            GI_AT.set(now);
            GI.set(1);
        }
        1 if now >= GI_AT.get() + 10 => {
            GI_AT.set(now);
            GI.set(2);
            if !order_infantry_deploy(gi) {
                gi_finish();
            }
        }
        2 if now >= GI_AT.get() + 60 => {
            if GI_ASKED.get() == 1 {
                if get(gi, Prop::Deployed) != Ok(0) {
                    bench_fail("movement.DeployInfantry", "the GI deployed after the veto");
                    gi_finish();
                    return;
                }
                GI_AT.set(now);
                GI.set(3);
                if !alive(gi) || !order_infantry_deploy(gi) {
                    gi_finish();
                }
            } else if now > GI_AT.get() + 200 || !alive(gi) {
                gi_finish();
            }
        }
        3 => {
            if GI_DEPLOYED.get() == 1 && GI_ALLOW.get() == 1 && get(gi, Prop::Deployed) == Ok(1) {
                mark("movement.DeployInfantry");
                mark("movement.DeployAllowed");
                gi_finish();
            } else if now > GI_AT.get() + 300 || !alive(gi) {
                gi_finish();
            }
        }
        _ => {}
    }
}

fn order_infantry_deploy(gi: Techno) -> bool {
    let units = [gi];
    enemy()
        .order(&Order::new(Command::ToggleInfantryDeploy, 0, 0, &units))
        .is_ok()
}

fn gi_finish() {
    let gi = U_GI.unit();
    if alive(gi) {
        let _ = Action::remove(gi);
    }
    disable(&TR_GI_DA);
    disable(&TR_GI_DEPLOYED);
    GI.set(DONE);
}

fn decide_gi_deploy(mut d: DeployAllowedDecision) {
    if d.object().0 != U_GI.get() {
        return;
    }
    match GI.get() {
        1 | 2 => {
            d.set_result(0);
            GI_ASKED.set(1);
        }
        3 => {
            d.set_result(1);
            GI_ALLOW.set(1);
        }
        _ => {}
    }
}

fn gi_deployed(record: &EventRecord) {
    if record.subject != U_GI.get() || record.value != 1 {
        return;
    }
    match GI.get() {
        1 | 2 => {
            bench_fail("movement.DeployInfantry", "deployed after the veto");
            gi_finish();
        }
        3 => GI_DEPLOYED.set(1),
        _ => {}
    }
}

// ------------------------------------------------- DeployMcv, BirthYard

/// The frame after the arena opened when the MCV scene starts even with
/// `HarvestCell` still pending (the scene takes at most ~1000 frames).
const MCV_LATEST: i64 = 2500;

/// The MCVs tried, with the yard each deploys into.
const MCVS: [(&str, &str); 2] = [("AMCV", "GACNST"), ("SMCV", "NACNST")];

/// One page of `Placeable` cells for `yard` in the 15x15 block around
/// the computer's building number `MC_BUILDING` (creation order), from
/// `MC_AFTER` on; the first whose 4x4 foundation is level takes `mcv` on
/// its yard's deploy cell (one cell in from the corner,
/// `deploy_origin_from_unit_cell`). A building whose block is scanned moves
/// the search to the next one; after the last, a round ends (`MC_TRIES`).
/// The MCV, or none.
fn place_mcv(mcv: TypeRef, yard: TypeRef) -> Option<Techno> {
    let mut buildings = [NO_HANDLE; 48];
    let count = Find::objects()
        .owned_by(enemy())
        .where_prop(Prop::Category, Op::Eq, Category::Building as i64)
        .page(&mut buildings, 0)
        .map_or(0, |(count, _)| count);
    let index = MC_BUILDING.get() as usize;
    if index >= count {
        MC_BUILDING.set(0);
        MC_AFTER.set(0);
        MC_TRIES.add(1);
        return None;
    }
    let Ok(base) = get(Techno(buildings[index]), Prop::Cell) else {
        MC_BUILDING.add(1);
        return None;
    };
    let mut out = [NO_HANDLE; 16];
    let found = Find::cells()
        .within(offset(Cell(base), -7, -7), 15, 15)
        .of_type(yard)
        .seen_by(enemy())
        .where_prop(Prop::Placeable, Op::Eq, 1)
        .page(&mut out, MC_AFTER.get());
    let (found, next) = found.unwrap_or((0, 0));
    MC_AFTER.set(next);
    if next == 0 {
        MC_BUILDING.add(1);
    }
    for &corner in &out[..found] {
        let corner = Cell(corner);
        let Ok(level) = corner.height() else {
            continue;
        };
        let level_ground =
            (0..4).all(|y| (0..4).all(|x| offset(corner, x, y).height().is_ok_and(|h| h == level)));
        if !level_ground {
            continue;
        }
        let at = offset(corner, 1, 1);
        match Action::create_object(at, mcv, enemy()) {
            Ok(unit) if cell_of(unit) == at => return Some(unit),
            Ok(unit) => {
                let _ = Action::remove(unit);
            }
            Err(_) => {}
        }
    }
    None
}

/// Spawns the first known MCV of `MCVS` on a placeable spot and binds its
/// triggers; false while none is found yet.
fn next_mcv() -> bool {
    let Some((mcv, yard)) = MCVS.into_iter().find_map(|(mcv, yard)| {
        Some((
            TypeRef::techno_type(mcv).ok()?,
            TypeRef::techno_type(yard).ok()?,
        ))
    }) else {
        return false;
    };
    if let Some(unit) = place_mcv(mcv, yard) {
        U_MCV.set(unit.0);
        TR_MCV_DA.set(decider(Event::DEPLOY_ALLOWED, unit, KEY_MCV_DA));
        TR_MCV_DEPLOYED.set(
            Trigger::on(Event::DEPLOYED)
                .bind(unit)
                .enqueue(KEY_MCV_DEPLOYED)
                .done()
                .map_or(0, |trigger| trigger.0),
        );
        TR_BIRTH_YARD.set(
            Trigger::on(Event::BIRTH)
                .bind(yard)
                .enqueue(KEY_BIRTH_YARD)
                .done()
                .map_or(0, |trigger| trigger.0),
        );
        return true;
    }
    false
}

/// The `Forces` claim on the computer house (`Domain` bit 1), added to or
/// taken from the bits this module holds there.
const FORCES: i64 = 2;

fn claim_forces(on: bool) {
    let them = enemy();
    let held = get(them, Prop::ControlDomains).unwrap_or(0);
    let want = if on { held | FORCES } else { held & !FORCES };
    if set(them, Prop::ControlDomains, want).is_ok() {
        MC_CLAIM.set(i64::from(on));
    }
}

/// The MCV's deploy is refused (it must be asked, no `Deployed` may come),
/// then allowed under a `Forces` claim: `Deployed` (`other` the yard) and
/// the yard's `Birth` (`other` the MCV) must come.
fn deploy_mcv(now: i64, since: i64) {
    let mcv = U_MCV.unit();
    match MC.get() {
        // After `HarvestCell` (or late in the run): the deploy re-anchors
        // the computer's base and Paranoid, which may change where its
        // harvesters go next.
        0 if since >= 40 && now >= MC_AT.get() && (HC.get() == DONE || since >= MCV_LATEST) => {
            let rounds = MC_TRIES.get();
            if next_mcv() {
                MC_AT.set(now);
                MC.set(1);
            } else if MC_TRIES.get() >= 3 {
                MC.set(DONE);
            } else if MC_TRIES.get() > rounds {
                // Every building's block scanned without a spot: again a
                // little later (the base and its units change).
                MC_AT.set(now + 60);
            }
        }
        1 if now >= MC_AT.get() + 15 => {
            MC_AT.set(now);
            MC.set(2);
            if !order_deploy(mcv) {
                mcv_retry(now);
            }
        }
        2 if now >= MC_AT.get() + 90 => {
            if MC_ASKED.get() == 1 && alive(mcv) {
                claim_forces(true);
                MC_AT.set(now);
                MC.set(3);
                if !order_deploy(mcv) {
                    mcv_retry(now);
                }
            } else if now > MC_AT.get() + 300 || !alive(mcv) {
                mcv_retry(now);
            }
        }
        3 => {
            let yard = MC_YARD.get();
            if yard != 0 && MC_BORN.get() != 0 {
                if MC_ALLOW.get() == 1 {
                    mark("movement.DeployMcv");
                    mark("movement.DeployAllowed");
                }
                if MC_BORN.get() == yard {
                    mark("movement.BirthYard");
                } else {
                    bench_fail("movement.BirthYard", "the Birth named another yard");
                }
                mcv_finish();
            } else if now > MC_AT.get() + 300 {
                mcv_retry(now);
            }
        }
        _ => {}
    }
}

/// A scene that stopped short: it starts again on a new spot,
/// three runs at most.
fn mcv_retry(now: i64) {
    mcv_cleanup();
    MC_RUNS.add(1);
    if MC_RUNS.get() >= 3 {
        MC.set(DONE);
        return;
    }
    for slot in [&MC_ASKED, &MC_ALLOW, &MC_YARD, &MC_BORN, &MC_TRIES] {
        slot.set(0);
    }
    MC_AT.set(now + 30);
    MC.set(0);
}

fn mcv_finish() {
    mcv_cleanup();
    MC.set(DONE);
}

fn mcv_cleanup() {
    if MC_CLAIM.get() == 1 {
        claim_forces(false);
    }
    for unit in [Techno(MC_YARD.get()), U_MCV.unit()] {
        if unit.0 != 0 && alive(unit) {
            let _ = Action::remove(unit);
        }
    }
    disable(&TR_MCV_DA);
    disable(&TR_MCV_DEPLOYED);
    disable(&TR_BIRTH_YARD);
}

fn decide_mcv_deploy(mut d: DeployAllowedDecision) {
    if d.object().0 != U_MCV.get() {
        return;
    }
    match MC.get() {
        1 | 2 => {
            d.set_result(0);
            MC_ASKED.set(1);
        }
        3 => {
            d.set_result(1);
            MC_ALLOW.set(1);
        }
        _ => {}
    }
}

fn mcv_deployed(record: &EventRecord) {
    if record.subject != U_MCV.get() || record.value != 1 {
        return;
    }
    match MC.get() {
        1 | 2 => {
            bench_fail("movement.DeployMcv", "deployed after the veto");
            MC_YARD.set(record.other);
            mcv_finish();
        }
        3 if record.other != NO_HANDLE && record.other != 0 => MC_YARD.set(record.other),
        3 => {
            bench_fail("movement.DeployMcv", "Deployed named no yard");
            mcv_finish();
        }
        _ => {}
    }
}

fn yard_born(record: &EventRecord) {
    if record.other == U_MCV.get() && MC.get() == 3 {
        MC_BORN.set(record.subject);
    }
}

// --------------------------------------------------------- BirthSurvivor

/// The crew types a power plant's survivors can be (`AlliedCrew=`,
/// `SovietCrew=`, `ThirdCrew=`): its survivors' `Birth` binds them.
const CREW: [&str; 3] = ["E1", "E2", "INIT"];

/// Warheads tried in turn until one destroys a plant.
const WARHEADS: [&str; 3] = ["Super", "AP", "HE"];

/// A player power plant in the arena, destroyed with defenses read: one of
/// its survivors' `Birth` must name the plant as `other` and the player as
/// the survivor's owner. Up to eight plants; every survivor is removed.
fn birth_survivor(now: i64, since: i64) {
    match SV.get() {
        0 if since >= 600 => {
            TR_BIRTH_SURVIVOR.set(type_trigger(Event::BIRTH, &CREW, KEY_BIRTH_SURVIVOR));
            SV.set(if TR_BIRTH_SURVIVOR.get() == 0 {
                DONE
            } else {
                1
            });
        }
        1 => {
            let at = offset(arena(crate::MOVEMENT), 4, 1);
            let warhead = WARHEADS
                .get(SV_WARHEAD.get() as usize)
                .and_then(|name| TypeRef::warhead(name).ok());
            let Some(warhead) = warhead else {
                if (SV_WARHEAD.get() as usize) < WARHEADS.len() {
                    SV_WARHEAD.add(1);
                } else {
                    sv_finish();
                }
                return;
            };
            let plant = ["GAPOWR", "NAPOWR"]
                .into_iter()
                .find_map(|id| put(&U_PLANT, at, id, player()));
            let Some(plant) = plant else {
                sv_finish();
                return;
            };
            SV_AT.set(now);
            SV.set(2);
            let hit = Action::damage(plant, 100_000, warhead).is_ok();
            if !hit || plant.health().is_ok_and(|health| health > 0) {
                // This warhead does not kill a plant: the next one.
                let _ = Action::remove(plant);
                SV_WARHEAD.add(1);
                SV.set(1);
            }
        }
        2 => {
            if SV_SEEN.get() == 1 {
                mark("movement.BirthSurvivor");
                SV_AT.set(now);
                SV.set(3);
            } else if now > SV_AT.get() + 90 {
                let plant = U_PLANT.unit();
                if alive(plant) {
                    let _ = Action::remove(plant);
                }
                SV_TRIES.add(1);
                if SV_TRIES.get() < 8 {
                    SV.set(1);
                } else {
                    sv_finish();
                }
            }
        }
        // The other survivors' `Birth` records remove them meanwhile.
        3 if now > SV_AT.get() + 60 => sv_finish(),
        _ => {}
    }
}

fn sv_finish() {
    disable(&TR_BIRTH_SURVIVOR);
    SV.set(DONE);
}

fn survivor_born(record: &EventRecord) {
    let plant = U_PLANT.get();
    if plant == 0 || record.other != plant {
        return;
    }
    let survivor = Techno(record.subject);
    if survivor.owner() == Ok(player()) {
        SV_SEEN.set(1);
    } else {
        bench_fail("movement.BirthSurvivor", "the survivor has another owner");
    }
    if alive(survivor) {
        let _ = Action::remove(survivor);
    }
}

/// A delivered-event trigger on `kind` bound to `ids`' types; its handle or 0.
fn type_trigger(kind: i32, ids: &[&str], key: i32) -> i64 {
    let mut trigger = Trigger::on(kind);
    for id in ids {
        if let Ok(ty) = TypeRef::techno_type(id) {
            trigger = trigger.bind(ty);
        }
    }
    trigger.enqueue(key).done().map_or(0, |trigger| trigger.0)
}

// ------------------------------------- ProductionAllowed, ProductionQueued

/// The types the computer house builds (Soviet, and Allied in case):
/// buildings and units its factories start themselves, for
/// `ProductionQueued`.
const PROD_TYPES: [&str; 16] = [
    "NAPOWR", "NAREFN", "NAHAND", "NAWEAP", "NARADR", "NALASR", "HTNK", "HARV", "GAPOWR", "GAREFN",
    "GAPILE", "GAWEAP", "GAPILL", "HTK", "E2", "E1",
];

/// The `ProductionQueued` deciders on every known type of `PROD_TYPES`.
fn production_open() {
    let mut halves = [
        Trigger::on(Event::PRODUCTION_QUEUED),
        Trigger::on(Event::PRODUCTION_QUEUED),
    ];
    for (slot, id) in PROD_TYPES.iter().enumerate() {
        if let Ok(ty) = TypeRef::techno_type(id) {
            let half = &mut halves[slot / 8];
            *half = core::mem::replace(half, Trigger::on(0)).bind(ty);
        }
    }
    let [first, second] = halves;
    TR_PQ.set(first.decide_sync(KEY_PQ).map_or(0, |t| t.0));
    TR_PQ2.set(second.decide_sync(KEY_PQ).map_or(0, |t| t.0));
}

/// A decision trigger on `kind` bound to `ids`' types; its handle or 0.
fn type_decider(kind: i32, ids: &[&str], key: i32) -> i64 {
    let mut trigger = Trigger::on(kind);
    for id in ids {
        if let Ok(ty) = TypeRef::techno_type(id) {
            trigger = trigger.bind(ty);
        }
    }
    trigger.decide_sync(key).map_or(0, |trigger| trigger.0)
}

/// The computer house's objects of type `ty` (at most 64 counted).
fn count_of(ty: TypeRef) -> i64 {
    let mut out = [NO_HANDLE; 64];
    Find::objects()
        .of_type(ty)
        .owned_by(enemy())
        .page(&mut out, 0)
        .map_or(0, |(count, _)| count as i64)
}

/// `ProductionQueued` first: the first type the computer's factory starts
/// is refused until it asks again (nothing started), then allowed (one more
/// must be built). `ProductionAllowed` follows it
/// ([`production_allowed`]).
fn production(now: i64, since: i64) {
    // Money early, so the computer's factories keep starting things.
    if since == 5 {
        let _ = Action::add_money(enemy(), 10000);
    }
    production_allowed(now, since);
    let ty = TypeRef::new(NameKind::TechnoType, PROD_TYPE.get() as i32);
    match PQ.get() {
        0 if PQ_NO.get() >= 1 && PQ_AT.get() == 0 => {
            let _ = Action::add_money(enemy(), 5000);
            PQ_AT.set(now);
        }
        0 if PQ_NO.get() >= 2 => {
            PQ_COUNT.set(count_of(ty));
            PQ_AT.set(now);
            PQ.set(1);
        }
        0 if PQ_AT.get() > 0 && now > PQ_AT.get() + 1500 => {
            PQ.set(DONE);
        }
        1 => {
            let count = count_of(ty);
            if PQ_YES.get() >= 1 && count > PQ_COUNT.get() {
                mark("movement.ProductionQueued");
                PQ.set(DONE);
            } else if now > PQ_AT.get() + 2000 {
                PQ.set(DONE);
            }
        }
        _ => {}
    }
    if PQ.get() == DONE && PA.get() == DONE {
        disable(&TR_PQ);
        disable(&TR_PQ2);
    }
}

/// The types `ProductionAllowed` queues, with their factory's `Type` and
/// `Progress`: Soviet, which the computer house builds.
const PA_CANDIDATES: [(&str, Prop, Prop); 3] = [
    (
        "E2",
        Prop::Factory_Infantry_Type,
        Prop::Factory_Infantry_Progress,
    ),
    (
        "HTNK",
        Prop::Factory_Vehicle_Type,
        Prop::Factory_Vehicle_Progress,
    ),
    (
        "NAPOWR",
        Prop::Factory_Building_Type,
        Prop::Factory_Building_Progress,
    ),
];

/// The `Production` bit of `ControlDomains` (RFC 0001 6i).
const PRODUCTION: i64 = 1;
/// `RefuseReason` `Prerequisite` (appendix 8.3, enum 26).
const PREREQUISITE: i64 = 4;
/// Frames each phase of a candidate is watched.
const PA_WATCH: i64 = 60;

fn claim_production(on: bool) {
    let them = enemy();
    let held = get(them, Prop::ControlDomains).unwrap_or(0);
    let want = if on {
        held | PRODUCTION
    } else {
        held & !PRODUCTION
    };
    if set(them, Prop::ControlDomains, want).is_ok() {
        PA_CLAIM.set(i64::from(on));
    }
}

fn queue(ty: TypeRef) -> bool {
    enemy().order(&Order::queue_production(ty)).is_ok()
}

/// Forbid, queue (nothing under way), allow back, queue (under way): see
/// the module docs. A candidate that does not get through gives way to the
/// next; the claim goes once the last is done.
fn production_allowed(now: i64, since: i64) {
    let Some(&(name, ty_prop, progress)) = PA_CANDIDATES.get(PA_CANDIDATE.get() as usize) else {
        if PA.get() != DONE {
            pa_end();
        }
        return;
    };
    let kind = TypeRef::techno_type(name).ok();
    let reads = |prop: Prop| get(enemy(), prop).unwrap_or(0);
    match PA.get() {
        // After the scenes on the computer's own AI (its claim stops a
        // harvester's return to Harvest and the AI's MCV deploys), or late.
        0 if (PQ.get() == DONE && HC.get() == DONE && MC.get() == DONE) || since >= 3200 => {
            let Some(kind) = kind else {
                return pa_next();
            };
            // The AI's own build of it would read the same.
            if reads(ty_prop) == kind.0 {
                return pa_next();
            }
            if PA_CLAIM.get() == 0 {
                claim_production(true);
                let _ = Action::add_money(enemy(), 5000);
            }
            PA_TYPE.set(i64::from(kind.id()));
            TR_PA.set(
                Trigger::on(Event::PRODUCTION_ALLOWED)
                    .bind(kind)
                    .decide_sync(KEY_PA)
                    .map_or(0, |t| t.0),
            );
            PA_AT.set(now);
            PA.set(1);
        }
        // Forbidden at a step: queue it.
        1 if PA_NO.get() >= 1 => {
            PA_REFUSED.set(0);
            PA_AT.set(now);
            PA.set(2);
            if kind.is_none_or(|kind| !queue(kind)) {
                pa_next();
            }
        }
        1 if now > PA_AT.get() + PA_WATCH => pa_next(),
        2 => {
            let Some(kind) = kind else {
                return pa_next();
            };
            if reads(ty_prop) == kind.0 {
                bench_fail(
                    "movement.ProductionAllowed",
                    "a forbidden type got under way",
                );
                return pa_end();
            }
            let refused = PA_REFUSED.get() - 1;
            if refused >= 0 && refused != PREREQUISITE {
                return pa_next();
            }
            if now >= PA_AT.get() + PA_WATCH {
                PA_AT.set(now);
                PA.set(3);
            }
        }
        // Allowed back at a step (its native answer, which must be 1):
        // queue it again.
        3 if PA_YES.get() >= 1 => {
            if PA_NATIVE.get() == 0 {
                return pa_next();
            }
            PA_REFUSED.set(0);
            PA_AT.set(now);
            PA.set(4);
            if kind.is_none_or(|kind| !queue(kind)) {
                pa_next();
            }
        }
        3 if now > PA_AT.get() + PA_WATCH => pa_next(),
        4 => {
            let Some(kind) = kind else {
                return pa_next();
            };
            if reads(ty_prop) == kind.0 && reads(progress) >= 1 {
                mark("movement.ProductionAllowed");
                let _ = enemy().order(&Order::cancel_production(kind));
                pa_end();
            } else if PA_REFUSED.get() != 0 || now > PA_AT.get() + PA_WATCH {
                pa_next();
            }
        }
        _ => {}
    }
}

/// The next candidate: this one's decider goes.
fn pa_next() {
    disable(&TR_PA);
    for slot in [&PA_NO, &PA_NATIVE, &PA_YES, &PA_REFUSED] {
        slot.set(0);
    }
    PA_TYPE.set(-1);
    PA_CANDIDATE.add(1);
    PA.set(0);
}

fn pa_end() {
    disable(&TR_PA);
    PA_TYPE.set(-1);
    if PA_CLAIM.get() == 1 {
        claim_production(false);
    }
    PA.set(DONE);
}

fn decide_production_allowed(mut d: ProductionAllowedDecision) {
    if d.house() != enemy() || i64::from(d.techno_type().id()) != PA_TYPE.get() {
        return;
    }
    match PA.get() {
        1 | 2 => {
            d.set_result(0);
            PA_NO.add(1);
        }
        3 | 4 => {
            let native = d.native();
            d.set_result(native);
            PA_YES.add(1);
            PA_NATIVE.set(native);
        }
        _ => {}
    }
}

fn decide_production_queued(mut d: ProductionQueuedDecision) {
    if d.house() != enemy() {
        return;
    }
    let ty = i64::from(d.techno_type().id());
    if PQ.get() == 0 {
        if PROD_TYPE.get() < 0 {
            PROD_TYPE.set(ty);
        }
        if ty == PROD_TYPE.get() {
            d.set_result(0);
            PQ_NO.add(1);
            return;
        }
    }
    if PQ.get() == 1 && ty == PROD_TYPE.get() {
        d.set_result(1);
        PQ_YES.add(1);
    }
}

/// `OrderRefused` (global): the command check above.
fn order_refused(record: &EventRecord) {
    if record.subject == U_CMD.get()
        && record.subject != NO_HANDLE
        && CA.get() == 1
        && CA_VETO.get() == 1
        && record.value == Command::Move as i64
        && record.value2 == VETOED
    {
        CA.set(2);
        return;
    }
    if record.value == Command::QueueProduction as i64
        && i64::from(record.id) == PA_TYPE.get()
        && matches!(PA.get(), 2 | 4)
    {
        PA_REFUSED.set(record.value2 + 1);
    }
}
