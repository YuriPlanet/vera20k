// Area `movement` of the bench (see `index.ts`), the port of the Rust
// `movement.rs`. Marks are named `movement.<point>`.
//
// The production, command, movement, path and unit AI decisions, and the
// `Push` and `SetFacing` actions. Each point is a small state machine run
// from `frame`, answered in a decision handler and checked on a later frame
// (or from a delivered `Commanded`, `Deployed` or `OrderRefused`), on
// objects this area creates in its arena:
// - player tanks (`MTNK`): `MoveFrame` then `Push` then `SetFacing` on one,
//   `PathFound`, `IdleMission`, `MissionStep` and `ApproachPoint` (an
//   attack on a sleeping computer GI) on one each, driven with
//   `Action.setMission`;
// - computer-house objects, ordered through `House.order`: a Chrono Miner
//   for `CommandAllowed` and `DestinationSet`, a Siege Chopper on level
//   ground for `DeployAllowed`, a standing GI for `DeployInfantry`; each
//   removed once done;
// - a computer MCV in the computer's own base on a `Placeable` spot:
//   `DeployMcv` (its refusal, then the deploy and its `Deployed`) and
//   `BirthYard` (the yard's `Birth`, `other` the MCV), under a `Forces`
//   claim from the allowed order until the deploy;
// - a player power plant destroyed by `Action.damage`: `BirthSurvivor`;
// - the computer house's own harvesters (`HarvestCell`) and production
//   (`ProductionQueued`), then `ProductionAllowed` on the module's own
//   `QueueProduction` under a `Production` claim.
//
// A `MoveFrame` decider that never answers (the observer) reads the NavCom
// a `DestinationSet`, `ApproachPoint` or `HarvestCell` answer set.
//
// Each Rust key is a handler function here; every handler returns at once
// once the finale began, as the Rust `on_events` and `on_decide` skip the
// areas' records then.
import {
  Action, ApproachPointDecision, Category, Cell, Command, CommandAllowedDecision, DeployAllowedDecision,
  DestinationSetDecision, Event, EventRecord, Find, HarvestCellDecision, House, IdleMissionDecision,
  Mission, MissionStepDecision, MoveFrameDecision, NO_HANDLE, NameKind, Op, Order, PathFoundDecision, Prop,
  ProductionAllowedDecision, ProductionQueuedDecision, Rules, SpeedType, Status, Techno, Trigger, TriggerRef,
  TypeRef, Modifier, frame as currentFrame, get, lastStatus, set,
} from "@vera20k/sdk";
import { MOVEMENT, alive, arena, benchFail, enemy, mark, offset, player, spawn } from "./bench";
import { finaleStarted } from "./finale";

/** Every mark this area reaches in the bench's scene. */
export const MARKS: StaticArray<string> = [
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

/** `RefuseReason` `Vetoed` (appendix 8.3, enum 26). */
const VETOED: i64 = 9;
/** A finished state machine. */
const DONE: i64 = 99;

// ----------------------------------------------------------------- State

let openAt: i64 = 0;

// MoveFrame, then Push and SetFacing, on one tank.
let tMf: i64 = 0;
let trMf: i64 = 0;
let mf: i64 = 0;
let mfX: i64 = 0;
let mfY: i64 = 0;
let mfTries: i64 = 0;
let pu: i64 = 0;
let puAt: i64 = 0;
let puX: i64 = 0;
let puY: i64 = 0;
let puDx: i64 = 0;
let puDy: i64 = 0;
let sf: i64 = 0;
let sfAt: i64 = 0;
let sfTo: i64 = 0;

// PathFound.
let tPf: i64 = 0;
let pf: i64 = 0;
let pfAt: i64 = 0;
let pfStart: i64 = 0;
let pfGoal: i64 = 0;
let pfSide: i64 = 1;
let pfTries: i64 = 0;

// IdleMission.
let tIm: i64 = 0;
let im: i64 = 0;
let imAt: i64 = 0;
let imGoal: i64 = 0;
let imExpect: i64 = 0;
let imHome: i64 = 0;
let imTry: i64 = 0;

// MissionStep.
let tMs: i64 = 0;
let ms: i64 = 0;
let msAt: i64 = 0;
let msDest: i64 = 0;

// ApproachPoint: a player tank attacks a computer GI (weapons off, asleep)
// seven cells away, inside its sight and out of its range.
let tAp2: i64 = 0;
let ap2Target: i64 = 0;
let ap2: i64 = 0;
let ap2At: i64 = 0;
let ap2Cell: i64 = 0;
let ap2Seen: i64 = 0;

// The MoveFrame observer (never answers).
let trObs: i64 = 0;

// CommandAllowed, then DestinationSet, on one computer-house miner.
let uCmd: i64 = 0;
let ca: i64 = 0;
let caAt: i64 = 0;
let caVeto: i64 = 0;
let caAllow: i64 = 0;
let caTries: i64 = 0;
let ds: i64 = 0;
let dsAt: i64 = 0;
let dsSeen: i64 = 0;
let dsNative: i64 = 0;
let cmdNative: i64 = 0;
let cmdModule: i64 = 0;

// HarvestCell, on the computer house's own harvesters.
let hc: i64 = 0;
let hcAt: i64 = 0;
let hcUnit: i64 = 0;
let hcCell: i64 = 0;
let hcSeen: i64 = 0;
let hcOther: i64 = 0;
let hcBound: i64 = 0;
let hcTries: i64 = 0;

// DeployAllowed on a Siege Chopper.
let uDeploy: i64 = 0;
let trDa: i64 = 0;
let trDeployed: i64 = 0;
let da: i64 = 0;
let daAt: i64 = 0;
let daKind: i64 = 0;
let daAllow: i64 = 0;
let daBecame: i64 = 0;

// DeployInfantry: a standing computer GI.
let uGi: i64 = 0;
let trGiDa: i64 = 0;
let trGiDeployed: i64 = 0;
let gi: i64 = 0;
let giAt: i64 = 0;
let giAsked: i64 = 0;
let giAllow: i64 = 0;
let giDeployed: i64 = 0;

// DeployMcv and BirthYard: a computer MCV in the computer's base.
let uMcv: i64 = 0;
let trMcvDa: i64 = 0;
let trMcvDeployed: i64 = 0;
let trBirthYard: i64 = 0;
let mc: i64 = 0;
let mcAt: i64 = 0;
let mcAfter: i64 = 0;
let mcBuilding: i64 = 0;
let mcRuns: i64 = 0;
let mcTries: i64 = 0;
let mcAsked: i64 = 0;
let mcAllow: i64 = 0;
let mcYard: i64 = 0;
let mcBorn: i64 = 0;
let mcClaim: i64 = 0;

// BirthSurvivor: a destroyed player power plant.
let uPlant: i64 = 0;
let trBirthSurvivor: i64 = 0;
let sv: i64 = 0;
let svAt: i64 = 0;
let svTries: i64 = 0;
let svSeen: i64 = 0;
let svWarhead: i64 = 0;

// ProductionQueued, then ProductionAllowed, on the computer house's own
// production.
let trPa: i64 = 0;
let trPq: i64 = 0;
let trPq2: i64 = 0;
let prodType: i64 = -1;
let pq: i64 = 0;
let pqAt: i64 = 0;
let pqNo: i64 = 0;
let pqYes: i64 = 0;
let pqCount: i64 = 0;
let pa: i64 = 0;
let paAt: i64 = 0;
let paNo: i64 = 0;
let paNative: i64 = 0;
let paYes: i64 = 0;
let paType: i64 = -1;
let paCandidate: i64 = 0;
let paClaim: i64 = 0;
/** The latest `OrderRefused` reason for the queued type, plus one (0 none). */
let paRefused: i64 = 0;

// Buffers, made once: decision handlers allocate nothing.
const ONE_UNIT = new StaticArray<Techno>(1);
const ROUTE = new StaticArray<i32>(8);
const ORE_CELLS = new StaticArray<i64>(32);
/** The `HarvestCell` handler's search; `near` moves its centre each time. */
const ORE_FIND: Find = Find.cells().where(Prop.Ore, Op.Gt, 0);
const BUILDINGS = new StaticArray<i64>(48);
const SPOTS = new StaticArray<i64>(16);
const COUNTED = new StaticArray<i64>(64);

// --------------------------------------------------------------- Helpers

function cellOf(unit: Techno): Cell {
  const cell = get(unit, Prop.Cell);
  return lastStatus() == Status.Ok ? cell : NO_HANDLE;
}

/** Chebyshev distance in cells. */
function dist(a: Cell, b: Cell): i32 {
  return max(abs(Cell.x(a) - Cell.x(b)), abs(Cell.y(a) - Cell.y(b)));
}

function passable(cell: Cell): bool {
  const v = get(cell, SpeedType.passable(SpeedType.Track));
  return lastStatus() == Status.Ok && v != 0;
}

/** `cell`'s height, or −1 when it does not read (heights are never negative). */
function heightOf(cell: Cell): i64 {
  const h = get(cell, Prop.Height);
  return lastStatus() == Status.Ok ? h : -1;
}

/** The first of the `dx, dy` pairs `offsets` from `from` a tracked unit may drive on, else the first. */
function pick(from: Cell, offsets: StaticArray<i32>): Cell {
  for (let i = 0; i + 1 < offsets.length; i += 2) {
    const cell = offset(from, offsets[i], offsets[i + 1]);
    if (passable(cell)) return cell;
  }
  return offset(from, offsets[0], offsets[1]);
}

/** One unit, for an order (the array is shared). */
function units(unit: Techno): StaticArray<Techno> {
  ONE_UNIT[0] = unit;
  return ONE_UNIT;
}

/** A trigger's handle when `done` (or `decideSync`) answered `Ok`, 0 otherwise (as the Rust `map_or(0, ..)`). */
function made(trigger: TriggerRef): i64 {
  return lastStatus() == Status.Ok ? trigger : 0;
}

/** Disables trigger `t` when it is not 0; returns the new handle (0). */
function disabled(t: i64): i64 {
  if (t != 0) TriggerRef.setEnabled(t, false);
  return 0;
}

/** The current frame (this file's `frame` is the area hook). */
function tick(): i64 {
  return <i64>currentFrame();
}

function giveOrder(order: Order): bool {
  return House.order(enemy(), order) == Status.Ok;
}

// ------------------------------------------------------------------ Hooks

/** `vera_init`. */
export function init(): void {}

/** `on_scenario_start`. */
export function start(): void {}

const PICK_IM: StaticArray<i32> = [0, 3, 3, 0, 0, -3, -3, 0];
const PICK_MS: StaticArray<i32> = [3, -2, 3, 2, -3, 2, -3, -2];
const PICK_AP: StaticArray<i32> = [0, -2, 2, -2, -2, -2, 2, 0, -2, 0];
const PICK_NATIVE: StaticArray<i32> = [3, 0, -3, 0];
const PICK_MODULE: StaticArray<i32> = [0, 4, 0, -4];
const HARVESTERS: StaticArray<string> = ["HARV", "CMIN", "SMIN"];

/** The frame the arena opens: the houses and the arena are known. */
export function open(): void {
  openAt = tick();
  const a = arena(MOVEMENT);
  const me = player();
  const them = enemy();

  // The player's tanks, west half of the arena.
  tMf = spawn(offset(a, -6, -6), "MTNK", me);
  if (tMf != NO_HANDLE) trMf = made(Trigger.on(Event.MoveFrame).bind(tMf).decideSync(decideMoveFrame));
  tPf = spawn(offset(a, -2, -6), "MTNK", me);
  if (tPf != NO_HANDLE) {
    const at = cellOf(tPf);
    pfStart = at;
    pfGoal = offset(at, 0, 4);
    let side = 1;
    for (let s = 1; s >= -1; s -= 2) {
      if (passable(offset(at, s, 1)) && passable(offset(at, s, 2)) && passable(offset(at, s, 3))) {
        side = s;
        break;
      }
    }
    pfSide = <i64>side;
    Trigger.on(Event.PathFound).bind(tPf).decideSync(decidePath);
  }
  tIm = spawn(offset(a, -6, -1), "MTNK", me);
  if (tIm != NO_HANDLE) {
    imGoal = pick(cellOf(tIm), PICK_IM);
    Trigger.on(Event.IdleMission).bind(tIm).decideSync(decideIdle);
  }
  tMs = spawn(offset(a, -6, 4), "MTNK", me);
  if (tMs != NO_HANDLE) {
    msDest = pick(cellOf(tMs), PICK_MS);
    Trigger.on(Event.MissionStep).bind(tMs).decideSync(decideStep);
  }
  tAp2 = spawn(offset(a, 3, 7), "MTNK", me);
  if (tAp2 != NO_HANDLE) {
    const at = cellOf(tAp2);
    ap2Target = spawn(offset(at, 0, 7), "E1", them);
    if (ap2Target != NO_HANDLE) {
      Modifier.overrideWith(ap2Target, Prop.DisableWeapons, 1, -1);
      Action.setMission(ap2Target, Mission.Sleep, NO_HANDLE);
      ap2Cell = pick(at, PICK_AP);
      Trigger.on(Event.ApproachPoint).bind(tAp2).decideSync(decideApproach);
      observe(tAp2);
    }
  }

  // The computer house's objects, east half (unarmed or weapons off).
  uCmd = spawn(offset(a, 7, -6), "CMIN", them);
  if (uCmd != NO_HANDLE) {
    const at = cellOf(uCmd);
    cmdNative = pick(at, PICK_NATIVE);
    cmdModule = pick(at, PICK_MODULE);
    Trigger.on(Event.CommandAllowed).bind(uCmd).decideSync(decideCommand);
    Trigger.on(Event.DestinationSet).bind(uCmd).decideSync(decideDestination);
    observe(uCmd);
    Trigger.on(Event.Commanded).bind(uCmd).enqueue(onCommanded).done();
  }
  Trigger.on(Event.OrderRefused).enqueue(orderRefused).done();

  // The computer house's own production and harvesters.
  productionOpen();
  const harvest = Trigger.on(Event.HarvestCell);
  for (let i = 0; i < HARVESTERS.length; i++) {
    const ty = Rules.technoType(HARVESTERS[i]);
    if (ty != NO_HANDLE) harvest.bind(ty);
  }
  harvest.decideSync(decideHarvest);
}

/** Every later frame. */
export function frame(at: i32): void {
  const now = <i64>at;
  const since = now - openAt;
  moveFrame(since);
  pushAndFacing(now);
  pathFound(now, since);
  idleMission(now, since);
  missionStep(now, since);
  approachPointPlayer(now, since);
  commandAllowed(now, since);
  destinationSet(now);
  harvestCell(now, since);
  deployAllowed(now, since);
  deployInfantry(now, since);
  deployMcv(now, since);
  birthSurvivor(now, since);
  production(now, since);
}

// ------------------------------------------- MoveFrame, Push, SetFacing

function moveFrame(since: i64): void {
  if (mf == 0) {
    if (since >= 10 && alive(tMf)) mf = 1;
  } else if (mf == 1 || mf == 2) {
    if (since > 600) mf = DONE;
  } else if (mf == DONE) {
    trMf = disabled(trMf);
  }
}

/** A 16-lepton step of the resting tank; the next request's position must be the old one plus the step (one retry the other way). */
function decideMoveFrame(d: MoveFrameDecision): void {
  if (finaleStarted()) return;
  if (d.unit != tMf) return;
  const x = <i64>d.x;
  const y = <i64>d.y;
  if (mf == 1) {
    if (d.moving) return;
    const dx: i64 = mfTries == 0 ? 16 : -16;
    if (d.stepBy(<i32>dx, 0)) {
      mfX = x + dx;
      mfY = y;
      mf = 2;
    }
  } else if (mf == 2) {
    if (x == mfX && y == mfY) {
      mark("movement.MoveFrame");
      mf = DONE;
    } else if (mfTries == 0) {
      mfTries = 1;
      mf = 1;
    } else {
      benchFail("movement.MoveFrame", "the step did not land");
      mf = DONE;
    }
  }
}

const DIRS: StaticArray<i32> = [1, 0, -1, 0, 0, 1, 0, -1];

function pushAndFacing(now: i64): void {
  const tank = tMf;
  if (pu == 0 && mf == DONE) {
    if (!alive(tank)) {
      pu = DONE;
      return;
    }
    const at = cellOf(tank);
    let dirX = 1;
    let dirY = 0;
    for (let i = 0; i < DIRS.length; i += 2) {
      const dx = DIRS[i];
      const dy = DIRS[i + 1];
      if (passable(offset(at, dx, dy)) && passable(offset(at, dx * 2, dy * 2)) && passable(offset(at, dx * 3, dy * 3))) {
        dirX = dx;
        dirY = dy;
        break;
      }
    }
    const x = get(tank, Prop.X);
    const xOk = lastStatus() == Status.Ok;
    const y = get(tank, Prop.Y);
    if (!xOk || lastStatus() != Status.Ok) {
      pu = DONE;
      return;
    }
    if (Action.push(tank, offset(at, dirX * 3, dirY * 3), 128, 4) == Status.Ok) {
      puX = x;
      puY = y;
      puDx = <i64>dirX;
      puDy = <i64>dirY;
      puAt = now;
      pu = 1;
    } else {
      benchFail("movement.Push", "Push answered an error");
      pu = DONE;
    }
  } else if (pu == 1 && now >= puAt + 12) {
    const x = get(tank, Prop.X);
    const xOk = lastStatus() == Status.Ok;
    const y = get(tank, Prop.Y);
    if (!xOk || lastStatus() != Status.Ok) {
      pu = DONE;
      return;
    }
    const moved = (x - puX) * puDx + (y - puY) * puDy;
    if (moved >= 96) mark("movement.Push");
    else benchFail("movement.Push", "the tank did not move");
    pu = DONE;
  }
  if (sf == 0 && pu == DONE) {
    const facing = get(tank, Prop.Facing);
    if (lastStatus() != Status.Ok) {
      sf = DONE;
      return;
    }
    const to = (facing + 64) & 255;
    if (Action.setFacing(tank, <i32>to) == Status.Ok) {
      sfTo = to;
      sfAt = now;
      sf = 1;
    } else {
      benchFail("movement.SetFacing", "SetFacing answered an error");
      sf = DONE;
    }
  } else if (sf == 1) {
    const f = get(tank, Prop.Facing);
    let off: i64 = 128;
    if (lastStatus() == Status.Ok) off = (((f - sfTo) % 256) + 256) % 256;
    if (min(off, 256 - off) <= 3) {
      mark("movement.SetFacing");
      sf = DONE;
    } else if (now > sfAt + 120) {
      benchFail("movement.SetFacing", "the body did not turn");
      sf = DONE;
    }
  }
}

// ------------------------------------------------------------- PathFound

/**
 * Orders the tank four cells south; the first search answers no route (it
 * must stay in its cell), the next one a detour a column to the side (it
 * must drive through it).
 */
function pathFound(now: i64, since: i64): void {
  const tank = tPf;
  if (pf != DONE && pf != 0 && !alive(tank)) {
    pf = DONE;
    return;
  }
  if (pf == 0) {
    if (since >= 10 && alive(tank)) {
      if (Action.setMission(tank, Mission.Move, pfGoal) == Status.Ok) {
        pfAt = now;
        pf = 1;
      } else {
        pf = DONE;
      }
    }
  } else if (pf == 1 || pf == 3) {
    if (now > pfAt + 200) {
      pfTries++;
      if (pfTries > 2 || Action.setMission(tank, Mission.Move, pfGoal) != Status.Ok) pf = DONE;
      pfAt = now;
    }
  } else if (pf == 2) {
    if (now >= pfAt + 25) {
      if (cellOf(tank) != pfStart) {
        benchFail("movement.PathFound", "the tank left its cell without a route");
        pf = DONE;
      } else if (Action.setMission(tank, Mission.Move, pfGoal) == Status.Ok) {
        pfAt = now;
        pf = 3;
      } else {
        pf = DONE;
      }
    }
  } else if (pf == 4) {
    const at = cellOf(tank);
    const side = Cell.x(at) - Cell.x(pfStart);
    const down = Cell.y(at) - Cell.y(pfStart);
    if (<i64>side == pfSide && down >= 1 && down <= 3) {
      mark("movement.PathFound");
      pf = DONE;
    } else if (now > pfAt + 300) {
      benchFail("movement.PathFound", "the tank did not drive the module route");
      pf = DONE;
    }
  }
}

/** `div_euclid(256)` of a lepton coordinate. */
function cellCoord(v: i32): i32 {
  return v >= 0 ? v / 256 : -((-v + 255) / 256);
}

function decidePath(d: PathFoundDecision): void {
  if (finaleStarted()) return;
  if (d.unit != tPf) return;
  if (pf == 1) {
    d.noRoute();
    pfAt = tick();
    pf = 2;
  } else if (pf == 2) {
    d.noRoute();
  } else if (pf == 3) {
    const cx = cellCoord(d.x);
    const cy = cellCoord(d.y);
    if (cx != Cell.x(pfStart) || cy != Cell.y(pfStart)) return;
    const s = <i32>pfSide;
    ROUTE[0] = cx + s;
    ROUTE[1] = cy + 1;
    ROUTE[2] = cx + s;
    ROUTE[3] = cy + 2;
    ROUTE[4] = cx + s;
    ROUTE[5] = cy + 3;
    ROUTE[6] = cx;
    ROUTE[7] = cy + 4;
    if (d.setCells(ROUTE)) {
      pfAt = tick();
      pf = 4;
    }
  }
}

// ---------------------------------------------------------- IdleMission

/**
 * Moves the tank three cells; every idle request after the order is
 * answered (attempt 0: Guard and Area Guard swapped; attempt 1, back home:
 * Sleep), and the answer must become its mission.
 */
function idleMission(now: i64, since: i64): void {
  const tank = tIm;
  if (im == 0) {
    if (since >= 12 && alive(tank)) {
      imHome = cellOf(tank);
      im = 1;
      imAt = now;
      if (Action.setMission(tank, Mission.Move, imGoal) != Status.Ok) im = DONE;
    }
  } else if (im == 1) {
    if (now > imAt + 400) im = DONE;
  } else if (im == 2) {
    let mission = get(tank, Prop.Mission);
    if (lastStatus() != Status.Ok) mission = -1;
    if (mission == imExpect) {
      mark("movement.IdleMission");
      im = DONE;
    } else if (now > imAt + 300) {
      if (imTry == 0) {
        imTry = 1;
        imAt = now;
        im = 1;
        if (Action.setMission(tank, Mission.Move, imHome) != Status.Ok) im = DONE;
      } else {
        benchFail("movement.IdleMission", "the unit kept another mission");
        im = DONE;
      }
    }
  }
}

function decideIdle(d: IdleMissionDecision): void {
  if (finaleStarted()) return;
  if (d.unit != tIm || (im != 1 && im != 2)) return;
  if (im == 1) {
    const native = d.native;
    let answer: i64;
    if (imTry == 1) answer = <i64>Mission.Sleep;
    else if (native == <i64>Mission.AreaGuard) answer = <i64>Mission.Guard;
    else answer = <i64>Mission.AreaGuard;
    imExpect = answer;
    imAt = tick();
    im = 2;
  }
  d.setResult(imExpect);
}

// ---------------------------------------------------------- MissionStep

/** A guarding tank: the first Guard-family body's outcome gets the module's destination, where the tank must arrive. */
function missionStep(now: i64, since: i64): void {
  const tank = tMs;
  if (ms == 0) {
    if (since >= 12 && alive(tank)) {
      ms = 1;
      msAt = now;
      if (Action.setMission(tank, Mission.Guard, NO_HANDLE) != Status.Ok) ms = DONE;
    }
  } else if (ms == 1) {
    if (now > msAt + 300) ms = DONE;
  } else if (ms == 2) {
    if (dist(cellOf(tank), msDest) <= 1) {
      mark("movement.MissionStep");
      ms = DONE;
    } else if (now > msAt + 300) {
      benchFail("movement.MissionStep", "the tank did not reach the module cell");
      ms = DONE;
    }
  }
}

function decideStep(d: MissionStepDecision): void {
  if (finaleStarted()) return;
  if (d.unit != tMs || ms != 1) return;
  const m = d.mission;
  if (m != 1 && m != 5 && m != 6 && m != 11 && m != 15) return;
  if (d.step == null) {
    benchFail("movement.MissionStep", "no result area");
    ms = DONE;
    return;
  }
  if (d.setDestination(msDest)) {
    msAt = tick();
    ms = 2;
  }
}

// -------------------------------------------------------- ApproachPoint

/**
 * A player tank ordered (`SetMission` Attack) onto a sleeping computer GI
 * (weapons off) seven cells away: every approach answers a cell two cells
 * north, which must become its NavCom (seen by the observer) or where it
 * arrives.
 */
function approachPointPlayer(now: i64, since: i64): void {
  const tank = tAp2;
  const target = ap2Target;
  if (ap2 == 0) {
    if (since >= 15) {
      if (!alive(tank) || !alive(target)) {
        ap2 = DONE;
      } else if (Action.setMissionOn(tank, Mission.Attack, target) == Status.Ok) {
        ap2At = now;
        ap2 = 1;
      } else {
        ap2Finish();
      }
    }
  } else if (ap2 == 1) {
    if (now > ap2At + 300) ap2Finish();
  } else if (ap2 == 2) {
    if (ap2Seen == 1 || dist(cellOf(tank), ap2Cell) <= 1) {
      mark("movement.ApproachPoint");
      ap2Finish();
    } else if (now > ap2At + 300) {
      benchFail("movement.ApproachPoint", "the tank did not head for the module cell");
      ap2Finish();
    }
  }
}

function ap2Finish(): void {
  if (alive(tAp2)) Action.setMission(tAp2, Mission.Guard, NO_HANDLE);
  if (alive(ap2Target)) Action.remove(ap2Target);
  ap2 = DONE;
}

function decideApproach(d: ApproachPointDecision): void {
  if (finaleStarted()) return;
  if (d.unit == tAp2 && (ap2 == 1 || ap2 == 2)) {
    if (d.target != ap2Target || d.chosen == NO_HANDLE) {
      benchFail("movement.ApproachPoint", "the request named another target");
    }
    d.setCell(ap2Cell);
    if (ap2 == 1) {
      ap2At = tick();
      ap2 = 2;
    }
  }
}

// ------------------------------------------------- the MoveFrame observer

/**
 * Binds the observer (a `MoveFrame` decider that never answers) to `unit`:
 * its requests carry the unit's NavCom, the effect of a `DestinationSet`,
 * `ApproachPoint` or `HarvestCell` answer.
 */
function observe(unit: Techno): void {
  if (trObs == 0) trObs = made(Trigger.on(Event.MoveFrame).bind(unit).decideSync(decideObserve));
  else TriggerRef.bind(trObs, unit);
}

function decideObserve(d: MoveFrameDecision): void {
  if (finaleStarted()) return;
  const unit = d.unit;
  const navcom = d.destinationCell;
  if (unit == uCmd && ds == 2) {
    if (navcom == cmdModule) dsSeen = 1;
    else if (navcom == cmdNative) dsNative = 1;
  }
  if (unit == tAp2 && ap2 == 2 && navcom == ap2Cell) ap2Seen = 1;
  if (unit == hcUnit && hc == 2) {
    if (navcom == hcCell) hcSeen = 1;
    else if (navcom != NO_HANDLE) hcOther = 1;
  }
}

// ---------------------------------------- CommandAllowed, DestinationSet

/** A Move order is vetoed (its `OrderRefused` must say `Vetoed`), then a Guard order is allowed (its `Commanded` must come). */
function commandAllowed(now: i64, since: i64): void {
  const miner = uCmd;
  if (ca == 0) {
    if (since >= 5 && alive(miner)) {
      ca = 1;
      caAt = now;
      if (!giveOrder(Order.moveTo(units(miner), cmdNative))) ca = DONE;
    }
  } else if (ca == 1) {
    if (now > caAt + 60) {
      caTries++;
      caVeto = 0;
      caAt = now;
      if (caTries > 3 || !alive(miner) || !giveOrder(Order.moveTo(units(miner), cmdNative))) ca = DONE;
    }
  } else if (ca == 2) {
    ca = 3;
    caAt = now;
    if (!giveOrder(Order.guard(units(miner), NO_HANDLE))) ca = DONE;
  } else if (ca == 3) {
    if (now > caAt + 60) ca = DONE;
  }
}

function decideCommand(d: CommandAllowedDecision): void {
  if (finaleStarted()) return;
  if (d.unit != uCmd) return;
  const command = d.command;
  if (ca == 1 && command == <i32>Command.Move) {
    d.setResult(0);
    caVeto = 1;
  } else if (ca == 3 && command == <i32>Command.Guard) {
    d.setResult(1);
    caAllow = 1;
  }
}

function onCommanded(record: EventRecord): void {
  if (finaleStarted()) return;
  if (record.subject == uCmd && record.value == <i64>Command.Guard && ca == 3 && caAllow == 1) {
    mark("movement.CommandAllowed");
    ca = DONE;
  }
}

/**
 * After the commands: a Move order to one cell (three cells from where the
 * miner now stands) whose destination the module replaces with another;
 * the observer must see that cell as its NavCom (or it arrives).
 */
function destinationSet(now: i64): void {
  const miner = uCmd;
  if (ds == 0) {
    if (ca == DONE) {
      if (!alive(miner)) {
        ds = DONE;
        return;
      }
      // The miner may have driven off to harvest meanwhile.
      const at = cellOf(miner);
      cmdNative = pick(at, PICK_NATIVE);
      cmdModule = pick(at, PICK_MODULE);
      if (!giveOrder(Order.moveTo(units(miner), cmdNative))) {
        ds = DONE;
        return;
      }
      dsAt = now;
      ds = 1;
    }
  } else if (ds == 1) {
    if (now > dsAt + 200) ds = DONE;
  } else if (ds == 2) {
    if (dsSeen == 1 || dist(cellOf(miner), cmdModule) <= 1) {
      mark("movement.DestinationSet");
      ds = DONE;
    } else if (now > dsAt + 150 || !alive(miner)) {
      if (dsNative == 1) benchFail("movement.DestinationSet", "the native destination stayed");
      ds = DONE;
    }
  }
}

function decideDestination(d: DestinationSetDecision): void {
  if (finaleStarted()) return;
  if (d.object != uCmd || (ds != 1 && ds != 2)) return;
  if (d.destinationCell == cmdNative) {
    d.setResult(cmdModule);
    if (ds == 1) {
      dsAt = tick();
      ds = 2;
    }
  }
}

// ----------------------------------------------------------- HarvestCell

/**
 * Answered scans tried before giving up. The scans come when a computer
 * harvester looks for ore (the first one after ~1900 frames in the bench's
 * scene), so the area waits for them for the whole run.
 */
const HC_MAX_TRIES: i64 = 12;

/**
 * The computer house's own harvesters: one scan is answered with another
 * ore cell of the field, which the observer must see as the harvester's
 * NavCom (or it arrives). Up to `HC_MAX_TRIES` tries.
 */
function harvestCell(now: i64, since: i64): void {
  if (hc == 0) {
    if (since >= 3) {
      hc = 1;
      hcAt = now;
    }
  } else if (hc == 2) {
    const harvester = hcUnit;
    if (hcBound != harvester) {
      observe(harvester);
      hcBound = harvester;
    }
    if (hcSeen == 1 || dist(cellOf(harvester), hcCell) <= 1) {
      mark("movement.HarvestCell");
      hc = DONE;
    } else if (now > hcAt + 300 || !alive(harvester)) {
      hcTries++;
      if (hcTries < HC_MAX_TRIES) {
        hcAt = now;
        hc = 1;
      } else {
        if (hcOther == 1) benchFail("movement.HarvestCell", "the harvester headed for another cell");
        hc = DONE;
      }
    }
  }
}

function decideHarvest(d: HarvestCellDecision): void {
  if (finaleStarted()) return;
  if (hc != 1) return;
  const harvester = d.harvester;
  const owner = get(harvester, Prop.Owner);
  if (lastStatus() != Status.Ok || owner != enemy()) return;
  const scanned = d.scanned;
  if (scanned == NO_HANDLE) return;
  const page = ORE_FIND.near(scanned, 5 * 256).page(ORE_CELLS, 0);
  const count = lastStatus() == Status.Ok ? page.count : 0;
  // The farthest ore cell at least two cells out (the first of equals).
  let far: i64 = NO_HANDLE;
  let farDist = -1;
  for (let i = 0; i < count; i++) {
    const c = ORE_CELLS[i];
    const dd = dist(c, scanned);
    if (dd >= 2 && dd >= farDist) {
      // `max_by_key` keeps the last of equal maxima.
      far = c;
      farDist = dd;
    }
  }
  if (far == NO_HANDLE) return;
  d.setResult(far);
  hcUnit = harvester;
  hcCell = far;
  hcSeen = 0;
  hcOther = 0;
  hcAt = tick();
  hc = 2;
}

// --------------------------------------------------------- DeployAllowed

/**
 * The deployers tried in turn: a Siege Chopper (a simple deployer, asked
 * in its Unload mission). An MCV has its own scene in the computer's base.
 */
const DEPLOYERS: StaticArray<string> = ["SCHP"];

/** A cell whose 5x5 block around it is level and passable, near `center`. */
function flatSpot(center: Cell): Cell {
  for (let ring = 0; ring <= 3; ring++) {
    for (let dy = -ring; dy <= ring; dy++) {
      for (let dx = -ring; dx <= ring; dx++) {
        if (max(abs(dx), abs(dy)) != ring) continue;
        const spot = offset(center, dx, dy);
        const level = heightOf(spot);
        if (level < 0) continue;
        let flat = true;
        for (let y = -2; y <= 2 && flat; y++) {
          for (let x = -2; x <= 2; x++) {
            const cell = offset(spot, x, y);
            if (heightOf(cell) != level || !passable(cell)) {
              flat = false;
              break;
            }
          }
        }
        if (flat) return spot;
      }
    }
  }
  return center;
}

/** Spawns deployer `daKind` for the computer house and binds the deploy triggers to it; false when none is left. */
function nextDeployer(): bool {
  while (daKind < <i64>DEPLOYERS.length) {
    const spot = flatSpot(offset(arena(MOVEMENT), 7, 4));
    const unit = spawn(spot, DEPLOYERS[<i32>daKind], enemy());
    if (unit != NO_HANDLE) {
      uDeploy = unit;
      Modifier.overrideWith(unit, Prop.DisableWeapons, 1, -1);
      if (trDa == 0) {
        trDa = made(Trigger.on(Event.DeployAllowed).bind(unit).decideSync(decideDeploy));
        trDeployed = made(Trigger.on(Event.Deployed).bind(unit).enqueue(onDeployed).done());
      } else {
        TriggerRef.bind(trDa, unit);
        TriggerRef.bind(trDeployed, unit);
      }
      return true;
    }
    daKind++;
  }
  return false;
}

function orderDeploy(unit: Techno): bool {
  return giveOrder(Order.deployMcv(units(unit)));
}

/** The deploy is vetoed (no `Deployed` may follow), then allowed (the `Deployed` must come); a deployer never asked gives way to the next. */
function deployAllowed(now: i64, since: i64): void {
  const unit = uDeploy;
  if (da == 0) {
    if (since >= 20) {
      if (!nextDeployer()) {
        da = DONE;
        return;
      }
      da = 5;
      daAt = now;
    }
  } else if (da == 5) {
    // Let it settle (a chopper lands) before the order.
    if (now >= daAt + 15) {
      da = 1;
      daAt = now;
      orderDeploy(unit);
    }
  } else if (da == 1) {
    if (now > daAt + 150) {
      if (alive(unit)) Action.remove(unit);
      daKind++;
      da = 0;
    }
  } else if (da == 2) {
    if (now >= daAt + 30) {
      da = 3;
      daAt = now;
      if (!alive(unit) || !orderDeploy(unit)) da = DONE;
    }
  } else if (da == 3) {
    if (now > daAt + 200) da = 4;
  } else if (da == 4) {
    // Leave nothing of the computer's next to the player's start.
    const became = daBecame;
    if (became != NO_HANDLE && alive(became)) Action.remove(became);
    if (alive(unit)) Action.remove(unit);
    da = DONE;
  }
}

function decideDeploy(d: DeployAllowedDecision): void {
  if (finaleStarted()) return;
  if (d.object != uDeploy) return;
  if (da == 1) {
    d.setResult(0);
    daAt = tick();
    da = 2;
  } else if (da == 3) {
    d.setResult(1);
    daAllow = 1;
  }
}

function onDeployed(record: EventRecord): void {
  if (finaleStarted()) return;
  if (record.subject != uDeploy || record.value != 1) return;
  if (da == 2) {
    benchFail("movement.DeployAllowed", "deployed after the veto");
    da = DONE;
  } else if (da == 3 && daAllow == 1) {
    mark("movement.DeployAllowed");
    daBecame = record.other;
    da = 4;
  }
}

// -------------------------------------------------------- DeployInfantry

/**
 * A standing computer GI (weapons off, far from the player's tanks) is
 * ordered to deploy (`ToggleInfantryDeploy`, its Unload mission): the
 * deploy is refused (it must be asked, and the GI must stay standing),
 * then allowed (its `Deployed` must come and `Deployed` read 1).
 */
function deployInfantry(now: i64, since: i64): void {
  const unit = uGi;
  if (gi == 0) {
    if (since >= 30) {
      const made_ = spawn(offset(arena(MOVEMENT), 10, -1), "E1", enemy());
      if (made_ == NO_HANDLE) {
        gi = DONE;
        return;
      }
      uGi = made_;
      Modifier.overrideWith(made_, Prop.DisableWeapons, 1, -1);
      trGiDa = made(Trigger.on(Event.DeployAllowed).bind(made_).decideSync(decideGiDeploy));
      trGiDeployed = made(Trigger.on(Event.Deployed).bind(made_).enqueue(onGiDeployed).done());
      giAt = now;
      gi = 1;
    }
  } else if (gi == 1) {
    if (now >= giAt + 10) {
      giAt = now;
      gi = 2;
      if (!orderInfantryDeploy(unit)) giFinish();
    }
  } else if (gi == 2) {
    if (now >= giAt + 60) {
      if (giAsked == 1) {
        const deployed = get(unit, Prop.Deployed);
        if (lastStatus() != Status.Ok || deployed != 0) {
          benchFail("movement.DeployInfantry", "the GI deployed after the veto");
          giFinish();
          return;
        }
        giAt = now;
        gi = 3;
        if (!alive(unit) || !orderInfantryDeploy(unit)) giFinish();
      } else if (now > giAt + 200 || !alive(unit)) {
        giFinish();
      }
    }
  } else if (gi == 3) {
    const deployed = get(unit, Prop.Deployed);
    const deployedOk = lastStatus() == Status.Ok && deployed == 1;
    if (giDeployed == 1 && giAllow == 1 && deployedOk) {
      mark("movement.DeployInfantry");
      mark("movement.DeployAllowed");
      giFinish();
    } else if (now > giAt + 300 || !alive(unit)) {
      giFinish();
    }
  }
}


function orderInfantryDeploy(unit: Techno): bool {
  return giveOrder(new Order(Command.ToggleInfantryDeploy, 0, 0, units(unit)));
}

function giFinish(): void {
  if (alive(uGi)) Action.remove(uGi);
  trGiDa = disabled(trGiDa);
  trGiDeployed = disabled(trGiDeployed);
  gi = DONE;
}

function decideGiDeploy(d: DeployAllowedDecision): void {
  if (finaleStarted()) return;
  if (d.object != uGi) return;
  if (gi == 1 || gi == 2) {
    d.setResult(0);
    giAsked = 1;
  } else if (gi == 3) {
    d.setResult(1);
    giAllow = 1;
  }
}

function onGiDeployed(record: EventRecord): void {
  if (finaleStarted()) return;
  if (record.subject != uGi || record.value != 1) return;
  if (gi == 1 || gi == 2) {
    benchFail("movement.DeployInfantry", "deployed after the veto");
    giFinish();
  } else if (gi == 3) {
    giDeployed = 1;
  }
}

// ------------------------------------------------- DeployMcv, BirthYard

/** The frame after the arena opened when the MCV scene starts even with `HarvestCell` still pending. */
const MCV_LATEST: i64 = 2500;

/** The MCVs tried, with the yard each deploys into. */
const MCV_NAMES: StaticArray<string> = ["AMCV", "SMCV"];
const YARD_NAMES: StaticArray<string> = ["GACNST", "NACNST"];

/**
 * One page of `Placeable` cells for `yard` in the 15x15 block around the
 * computer's building number `mcBuilding`, from `mcAfter` on; the first
 * whose 4x4 foundation is level takes `mcv` on its yard's deploy cell (one
 * cell in from the corner). A building whose block is scanned moves the
 * search to the next one; after the last, a round ends (`mcTries`). The
 * MCV, or `NO_HANDLE`.
 */
function placeMcv(mcv: TypeRef, yard: TypeRef): Techno {
  const page = Find.objects()
    .ownedBy(enemy())
    .where(Prop.Category, Op.Eq, <i64>Category.Building)
    .page(BUILDINGS, 0);
  const count = lastStatus() == Status.Ok ? page.count : 0;
  const index = <i32>mcBuilding;
  if (index >= count) {
    mcBuilding = 0;
    mcAfter = 0;
    mcTries++;
    return NO_HANDLE;
  }
  const baseCell = get(BUILDINGS[index], Prop.Cell);
  if (lastStatus() != Status.Ok) {
    mcBuilding++;
    return NO_HANDLE;
  }
  const spots = Find.cells()
    .within(offset(baseCell, -7, -7), 15, 15)
    .ofType(yard)
    .seenBy(enemy())
    .where(Prop.Placeable, Op.Eq, 1)
    .page(SPOTS, mcAfter);
  let found = 0;
  let next: i64 = 0;
  if (lastStatus() == Status.Ok) {
    found = spots.count;
    next = spots.next;
  }
  mcAfter = next;
  if (next == 0) mcBuilding++;
  for (let i = 0; i < found; i++) {
    const corner = SPOTS[i];
    const level = heightOf(corner);
    if (level < 0) continue;
    let levelGround = true;
    for (let y = 0; y < 4 && levelGround; y++) {
      for (let x = 0; x < 4; x++) {
        if (heightOf(offset(corner, x, y)) != level) {
          levelGround = false;
          break;
        }
      }
    }
    if (!levelGround) continue;
    const at = offset(corner, 1, 1);
    const unit = Action.createObject(at, mcv, enemy());
    if (unit != NO_HANDLE && lastStatus() == Status.Ok) {
      if (cellOf(unit) == at) return unit;
      Action.remove(unit);
    }
  }
  return NO_HANDLE;
}

/** Spawns the first known MCV on a placeable spot and binds its triggers; false while none is found yet. */
function nextMcv(): bool {
  let mcv: TypeRef = NO_HANDLE;
  let yard: TypeRef = NO_HANDLE;
  for (let i = 0; i < MCV_NAMES.length; i++) {
    const m = Rules.technoType(MCV_NAMES[i]);
    if (m == NO_HANDLE) continue;
    const y = Rules.technoType(YARD_NAMES[i]);
    if (y == NO_HANDLE) continue;
    mcv = m;
    yard = y;
    break;
  }
  if (mcv == NO_HANDLE) return false;
  const unit = placeMcv(mcv, yard);
  if (unit == NO_HANDLE) return false;
  uMcv = unit;
  trMcvDa = made(Trigger.on(Event.DeployAllowed).bind(unit).decideSync(decideMcvDeploy));
  trMcvDeployed = made(Trigger.on(Event.Deployed).bind(unit).enqueue(onMcvDeployed).done());
  trBirthYard = made(Trigger.on(Event.Birth).bind(yard).enqueue(onYardBorn).done());
  return true;
}

/** The `Forces` claim on the computer house (`Domain` bit 1). */
const FORCES: i64 = 2;

function claimForces(on: bool): void {
  const them = enemy();
  let held = get(them, Prop.ControlDomains);
  if (lastStatus() != Status.Ok) held = 0;
  const want = on ? held | FORCES : held & ~FORCES;
  if (set(them, Prop.ControlDomains, want) == Status.Ok) mcClaim = on ? 1 : 0;
}

/**
 * The MCV's deploy is refused (it must be asked, no `Deployed` may come),
 * then allowed under a `Forces` claim: `Deployed` (`other` the yard) and
 * the yard's `Birth` (`other` the MCV) must come.
 */
function deployMcv(now: i64, since: i64): void {
  const mcv = uMcv;
  if (mc == 0) {
    // After `HarvestCell` (or late in the run).
    if (since >= 40 && now >= mcAt && (hc == DONE || since >= MCV_LATEST)) {
      const rounds = mcTries;
      if (nextMcv()) {
        mcAt = now;
        mc = 1;
      } else if (mcTries >= 3) {
        mc = DONE;
      } else if (mcTries > rounds) {
        // Every building's block scanned without a spot: again a little later.
        mcAt = now + 60;
      }
    }
  } else if (mc == 1) {
    if (now >= mcAt + 15) {
      mcAt = now;
      mc = 2;
      if (!orderDeploy(mcv)) mcvRetry(now);
    }
  } else if (mc == 2) {
    if (now >= mcAt + 90) {
      if (mcAsked == 1 && alive(mcv)) {
        claimForces(true);
        mcAt = now;
        mc = 3;
        if (!orderDeploy(mcv)) mcvRetry(now);
      } else if (now > mcAt + 300 || !alive(mcv)) {
        mcvRetry(now);
      }
    }
  } else if (mc == 3) {
    const yard = mcYard;
    if (yard != 0 && mcBorn != 0) {
      if (mcAllow == 1) {
        mark("movement.DeployMcv");
        mark("movement.DeployAllowed");
      }
      if (mcBorn == yard) mark("movement.BirthYard");
      else benchFail("movement.BirthYard", "the Birth named another yard");
      mcvFinish();
    } else if (now > mcAt + 300) {
      mcvRetry(now);
    }
  }
}

/** A scene that stopped short: it starts again on a new spot, three runs at most. */
function mcvRetry(now: i64): void {
  mcvCleanup();
  mcRuns++;
  if (mcRuns >= 3) {
    mc = DONE;
    return;
  }
  mcAsked = 0;
  mcAllow = 0;
  mcYard = 0;
  mcBorn = 0;
  mcTries = 0;
  mcAt = now + 30;
  mc = 0;
}

function mcvFinish(): void {
  mcvCleanup();
  mc = DONE;
}

function mcvCleanup(): void {
  if (mcClaim == 1) claimForces(false);
  if (mcYard != 0 && alive(mcYard)) Action.remove(mcYard);
  if (uMcv != 0 && alive(uMcv)) Action.remove(uMcv);
  trMcvDa = disabled(trMcvDa);
  trMcvDeployed = disabled(trMcvDeployed);
  trBirthYard = disabled(trBirthYard);
}

function decideMcvDeploy(d: DeployAllowedDecision): void {
  if (finaleStarted()) return;
  if (d.object != uMcv) return;
  if (mc == 1 || mc == 2) {
    d.setResult(0);
    mcAsked = 1;
  } else if (mc == 3) {
    d.setResult(1);
    mcAllow = 1;
  }
}

function onMcvDeployed(record: EventRecord): void {
  if (finaleStarted()) return;
  if (record.subject != uMcv || record.value != 1) return;
  if (mc == 1 || mc == 2) {
    benchFail("movement.DeployMcv", "deployed after the veto");
    mcYard = record.other;
    mcvFinish();
  } else if (mc == 3) {
    if (record.other != NO_HANDLE) {
      mcYard = record.other;
    } else {
      benchFail("movement.DeployMcv", "Deployed named no yard");
      mcvFinish();
    }
  }
}

function onYardBorn(record: EventRecord): void {
  if (finaleStarted()) return;
  if (record.other == uMcv && mc == 3) mcBorn = record.subject;
}

// --------------------------------------------------------- BirthSurvivor

/** The crew types a power plant's survivors can be: its survivors' `Birth` binds them. */
const CREW: StaticArray<string> = ["E1", "E2", "INIT"];

/** Warheads tried in turn until one destroys a plant. */
const WARHEADS: StaticArray<string> = ["Super", "AP", "HE"];
const PLANTS: StaticArray<string> = ["GAPOWR", "NAPOWR"];

/**
 * A player power plant in the arena, destroyed with defenses read: one of
 * its survivors' `Birth` must name the plant as `other` and the player as
 * the survivor's owner. Up to eight plants; every survivor is removed.
 */
function birthSurvivor(now: i64, since: i64): void {
  if (sv == 0) {
    if (since >= 600) {
      const trigger = Trigger.on(Event.Birth);
      for (let i = 0; i < CREW.length; i++) {
        const ty = Rules.technoType(CREW[i]);
        if (ty != NO_HANDLE) trigger.bind(ty);
      }
      trBirthSurvivor = made(trigger.enqueue(onSurvivorBorn).done());
      sv = trBirthSurvivor == 0 ? DONE : 1;
    }
  } else if (sv == 1) {
    const at = offset(arena(MOVEMENT), 4, 1);
    let warhead: TypeRef = NO_HANDLE;
    if (svWarhead < <i64>WARHEADS.length) warhead = Rules.warhead(WARHEADS[<i32>svWarhead]);
    if (warhead == NO_HANDLE) {
      if (svWarhead < <i64>WARHEADS.length) svWarhead++;
      else svFinish();
      return;
    }
    let plant: Techno = NO_HANDLE;
    for (let i = 0; i < PLANTS.length; i++) {
      plant = spawn(at, PLANTS[i], player());
      if (plant != NO_HANDLE) {
        uPlant = plant;
        break;
      }
    }
    if (plant == NO_HANDLE) {
      svFinish();
      return;
    }
    svAt = now;
    sv = 2;
    const hit = Action.damage(plant, 100000, warhead) == Status.Ok;
    const health = get(plant, Prop.Health);
    if (!hit || (lastStatus() == Status.Ok && health > 0)) {
      // This warhead does not kill a plant: the next one.
      Action.remove(plant);
      svWarhead++;
      sv = 1;
    }
  } else if (sv == 2) {
    if (svSeen == 1) {
      mark("movement.BirthSurvivor");
      svAt = now;
      sv = 3;
    } else if (now > svAt + 90) {
      if (alive(uPlant)) Action.remove(uPlant);
      svTries++;
      if (svTries < 8) sv = 1;
      else svFinish();
    }
  } else if (sv == 3) {
    // The other survivors' `Birth` records remove them meanwhile.
    if (now > svAt + 60) svFinish();
  }
}

function svFinish(): void {
  trBirthSurvivor = disabled(trBirthSurvivor);
  sv = DONE;
}

function onSurvivorBorn(record: EventRecord): void {
  if (finaleStarted()) return;
  const plant = uPlant;
  if (plant == 0 || record.other != plant) return;
  const survivor = record.subject;
  const owner = get(survivor, Prop.Owner);
  if (lastStatus() == Status.Ok && owner == player()) svSeen = 1;
  else benchFail("movement.BirthSurvivor", "the survivor has another owner");
  if (alive(survivor)) Action.remove(survivor);
}

// ------------------------------------- ProductionAllowed, ProductionQueued

/** The types the computer house builds (Soviet, and Allied in case), for `ProductionQueued`. */
const PROD_TYPES: StaticArray<string> = [
  "NAPOWR", "NAREFN", "NAHAND", "NAWEAP", "NARADR", "NALASR", "HTNK", "HARV", "GAPOWR", "GAREFN",
  "GAPILE", "GAWEAP", "GAPILL", "HTK", "E2", "E1",
];

/** The `ProductionQueued` deciders on every known type of `PROD_TYPES`, eight bindings each. */
function productionOpen(): void {
  const first = Trigger.on(Event.ProductionQueued);
  const second = Trigger.on(Event.ProductionQueued);
  for (let slot = 0; slot < PROD_TYPES.length; slot++) {
    const ty = Rules.technoType(PROD_TYPES[slot]);
    if (ty == NO_HANDLE) continue;
    if (slot / 8 == 0) first.bind(ty);
    else second.bind(ty);
  }
  trPq = made(first.decideSync(decideProductionQueued));
  trPq2 = made(second.decideSync(decideProductionQueued));
}

/** The computer house's objects of type `ty` (at most 64 counted). */
function countOf(ty: TypeRef): i64 {
  const page = Find.objects().ofType(ty).ownedBy(enemy()).page(COUNTED, 0);
  return lastStatus() == Status.Ok ? <i64>page.count : 0;
}

/**
 * `ProductionQueued` first: the first type the computer's factory starts
 * is refused until it asks again (nothing started), then allowed (one more
 * must be built). `ProductionAllowed` follows it.
 */
function production(now: i64, since: i64): void {
  // Money early, so the computer's factories keep starting things.
  if (since == 5) Action.addMoney(enemy(), 10000);
  productionAllowed(now, since);
  const ty = TypeRef.make(NameKind.TechnoType, <i32>prodType);
  if (pq == 0) {
    if (pqNo >= 1 && pqAt == 0) {
      Action.addMoney(enemy(), 5000);
      pqAt = now;
    } else if (pqNo >= 2) {
      pqCount = countOf(ty);
      pqAt = now;
      pq = 1;
    } else if (pqAt > 0 && now > pqAt + 1500) {
      pq = DONE;
    }
  } else if (pq == 1) {
    const count = countOf(ty);
    if (pqYes >= 1 && count > pqCount) {
      mark("movement.ProductionQueued");
      pq = DONE;
    } else if (now > pqAt + 2000) {
      pq = DONE;
    }
  }
  if (pq == DONE && pa == DONE) {
    trPq = disabled(trPq);
    trPq2 = disabled(trPq2);
  }
}

/** The types `ProductionAllowed` queues, with their factory's `Type` and `Progress`: Soviet, which the computer house builds. */
const PA_NAMES: StaticArray<string> = ["E2", "HTNK", "NAPOWR"];
const PA_TYPE_PROPS: StaticArray<i32> = [Prop.Factory_Infantry_Type, Prop.Factory_Vehicle_Type, Prop.Factory_Building_Type];
const PA_PROGRESS_PROPS: StaticArray<i32> = [
  Prop.Factory_Infantry_Progress, Prop.Factory_Vehicle_Progress, Prop.Factory_Building_Progress,
];

/** The `Production` bit of `ControlDomains` (RFC 0001 6i). */
const PRODUCTION: i64 = 1;
/** `RefuseReason` `Prerequisite` (appendix 8.3, enum 26). */
const PREREQUISITE: i64 = 4;
/** Frames each phase of a candidate is watched. */
const PA_WATCH: i64 = 60;

function claimProduction(on: bool): void {
  const them = enemy();
  let held = get(them, Prop.ControlDomains);
  if (lastStatus() != Status.Ok) held = 0;
  const want = on ? held | PRODUCTION : held & ~PRODUCTION;
  if (set(them, Prop.ControlDomains, want) == Status.Ok) paClaim = on ? 1 : 0;
}

function queue(ty: TypeRef): bool {
  return giveOrder(Order.queueProduction(ty));
}

/** A reading of the computer house, 0 when it does not read. */
function reads(prop: Prop): i64 {
  const v = get(enemy(), prop);
  return lastStatus() == Status.Ok ? v : 0;
}

/**
 * Forbid, queue (nothing under way), allow back, queue (under way). A
 * candidate that does not get through gives way to the next; the claim
 * goes once the last is done.
 */
function productionAllowed(now: i64, since: i64): void {
  if (paCandidate >= <i64>PA_NAMES.length) {
    if (pa != DONE) paEnd();
    return;
  }
  const index = <i32>paCandidate;
  const kind = Rules.technoType(PA_NAMES[index]);
  const tyProp = PA_TYPE_PROPS[index];
  const progress = PA_PROGRESS_PROPS[index];
  if (pa == 0) {
    // After the scenes on the computer's own AI, or late.
    if ((pq == DONE && hc == DONE && mc == DONE) || since >= 3200) {
      if (kind == NO_HANDLE) {
        paNext();
        return;
      }
      // The AI's own build of it would read the same.
      if (reads(tyProp) == kind) {
        paNext();
        return;
      }
      if (paClaim == 0) {
        claimProduction(true);
        Action.addMoney(enemy(), 5000);
      }
      paType = <i64>TypeRef.id(kind);
      trPa = made(Trigger.on(Event.ProductionAllowed).bind(kind).decideSync(decideProductionAllowed));
      paAt = now;
      pa = 1;
    }
  } else if (pa == 1) {
    // Forbidden at a step: queue it.
    if (paNo >= 1) {
      paRefused = 0;
      paAt = now;
      pa = 2;
      if (kind == NO_HANDLE || !queue(kind)) paNext();
    } else if (now > paAt + PA_WATCH) {
      paNext();
    }
  } else if (pa == 2) {
    if (kind == NO_HANDLE) {
      paNext();
      return;
    }
    if (reads(tyProp) == kind) {
      benchFail("movement.ProductionAllowed", "a forbidden type got under way");
      paEnd();
      return;
    }
    const refused = paRefused - 1;
    if (refused >= 0 && refused != PREREQUISITE) {
      paNext();
      return;
    }
    if (now >= paAt + PA_WATCH) {
      paAt = now;
      pa = 3;
    }
  } else if (pa == 3) {
    // Allowed back at a step (its native answer, which must be 1): queue it again.
    if (paYes >= 1) {
      if (paNative == 0) {
        paNext();
        return;
      }
      paRefused = 0;
      paAt = now;
      pa = 4;
      if (kind == NO_HANDLE || !queue(kind)) paNext();
    } else if (now > paAt + PA_WATCH) {
      paNext();
    }
  } else if (pa == 4) {
    if (kind == NO_HANDLE) {
      paNext();
      return;
    }
    if (reads(tyProp) == kind && reads(progress) >= 1) {
      mark("movement.ProductionAllowed");
      giveOrder(Order.cancelProduction(kind));
      paEnd();
    } else if (paRefused != 0 || now > paAt + PA_WATCH) {
      paNext();
    }
  }
}

/** The next candidate: this one's decider goes. */
function paNext(): void {
  trPa = disabled(trPa);
  paNo = 0;
  paNative = 0;
  paYes = 0;
  paRefused = 0;
  paType = -1;
  paCandidate++;
  pa = 0;
}

function paEnd(): void {
  trPa = disabled(trPa);
  paType = -1;
  if (paClaim == 1) claimProduction(false);
  pa = DONE;
}

function decideProductionAllowed(d: ProductionAllowedDecision): void {
  if (finaleStarted()) return;
  if (d.house != enemy() || <i64>TypeRef.id(d.technoType) != paType) return;
  if (pa == 1 || pa == 2) {
    d.setResult(0);
    paNo++;
  } else if (pa == 3 || pa == 4) {
    const native = d.native;
    d.setResult(native);
    paYes++;
    paNative = native;
  }
}

function decideProductionQueued(d: ProductionQueuedDecision): void {
  if (finaleStarted()) return;
  if (d.house != enemy()) return;
  const ty = <i64>TypeRef.id(d.technoType);
  if (pq == 0) {
    if (prodType < 0) prodType = ty;
    if (ty == prodType) {
      d.setResult(0);
      pqNo++;
      return;
    }
  }
  if (pq == 1 && ty == prodType) {
    d.setResult(1);
    pqYes++;
  }
}

/** `OrderRefused` (global): the command check above. */
function orderRefused(record: EventRecord): void {
  if (finaleStarted()) return;
  if (
    record.subject == uCmd && record.subject != NO_HANDLE && ca == 1 && caVeto == 1
    && record.value == <i64>Command.Move && record.value2 == VETOED
  ) {
    ca = 2;
    return;
  }
  if (record.value == <i64>Command.QueueProduction && <i64>record.id == paType && (pa == 2 || pa == 4)) {
    paRefused = record.value2 + 1;
  }
}
