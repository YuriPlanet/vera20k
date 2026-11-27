// One battle: where the raiders arrive, who attacks whom, and how it ends.
// (Owner: battle)
//
// Both sides stand held for a short face-off and then fight by themselves:
// every second each unit's target is looked over, with a preference for
// what its weapon can hurt. A champion fielded alone is surrounded:
// early waves are too small to stop some champions, and no champion is to
// hold a wave by itself.

import {
  Action, Cell, FIXED_ONE, Find, Mission, Modifier, NO_HANDLE, Op, Prop, Rules, SpeedType, Status,
  Techno, frame, get, lastStatus, log,
} from "@vera20k/sdk";
import * as content from "../../data/content";
import { FRONT_NAMES, MAX_COMPANY, MAX_RAIDERS } from "../../data/index";
import * as waves from "../../data/waves";
import * as perks from "./perks";
import * as state from "./state";
import { Line, say } from "./text";
import * as world from "./world";

/** Frames both sides look at each other. */
const FACE_OFF: i32 = 45;
/** Timer seconds of fighting (15 frames each). */
const FIGHT_SECONDS: i32 = 120;
/**
 * What the raiders' fire and armor are multiplied by against a champion
 * fielded alone, so that no champion clears a wave by itself.
 */
const SURROUND: i64 = 6 * FIXED_ONE;
/** Frames the field is left as it is once a side is gone. */
const AFTERMATH: i32 = 30;
/** Frames between two looks at one side's targets. */
const LOOK: i32 = 15;
/** Frames without a hit on either side after which everyone closes in. */
const STANDOFF: i32 = 120;
/** Frames that closing in is left to work before targets are looked at. */
const CLOSING: i32 = 45;
/** Frames between two counts of the living. */
const COUNT: i32 = 3;

const FACING_OFF: i64 = 0;
const FIGHTING: i64 = 1;
const WON: i64 = 2;
const LOST: i64 = 3;
const IDLE: i64 = 4;

/** `tick`'s results: nothing decided yet, a loss, a win. */
export const UNDECIDED: i32 = -1;
export const REPORT_LOST: i32 = 0;
export const REPORT_WON: i32 = 1;
/** The decided battle's kills, with `tick`'s report. */
export let reportKills: i64 = 0;

let stage: i64 = IDLE;
/** The frame the stage ends. */
let until: i64 = 0;
let arrivedCount: i64 = 0;
/** The health on the field at the last count, and when that last changed. */
let fieldHealth: i64 = 0;
let hitAt: i64 = 0;
/** No look at targets before this frame. */
let lookAt: i64 = 0;

const FIGHTERS: i32 = MAX_COMPANY + MAX_RAIDERS;
// Per fighter, the company first: its index in `NATURES`, whether it is a
// vehicle, and looks in a row it spent unable to get at its target.
const NATURE = new StaticArray<i64>(FIGHTERS);
const IS_VEHICLE = new StaticArray<i64>(FIGHTERS);
const STUCK = new StaticArray<i64>(FIGHTERS);

// -------------------------------------------------------------- Targets

// What a weapon is for.
const BOTH: i32 = 0;
const FOOT: i32 = 1;
/** All but harmless to armor. */
const FOOT_ONLY: i32 = 2;
const ARMOR: i32 = 3;
/** All but harmless to foot. */
const ARMOR_ONLY: i32 = 4;

// From the weapons' Range and Verses in `rulesmd.ini`. The first stands in
// for a type that is not listed. Its reach is in cells, rounded up.
const NATURE_ID: StaticArray<string> = [
  "", "E1", "E2", "INIT", "FLAKT", "HTK", "YTNK", "SNIPE", "GHOST", "SHK", "BRUTE", "TTNK", "SREF",
  "MTNK", "LTNK", "HTNK", "APOC", "TNKD",
];
const NATURE_SUITS: StaticArray<i32> = [
  BOTH, FOOT, FOOT, FOOT, FOOT, FOOT, FOOT, FOOT_ONLY, FOOT_ONLY, BOTH, BOTH, BOTH, BOTH,
  ARMOR, ARMOR, ARMOR, ARMOR, ARMOR_ONLY,
];
const NATURE_REACH: StaticArray<i32> = [6, 4, 4, 5, 5, 5, 6, 14, 6, 3, 2, 4, 10, 5, 5, 6, 6, 5];

// All in half cells.
const CROWDED: i32 = 3;
const CHANGE_OF_MIND: i32 = 6;
const BLOCKED: i32 = 40;

/** How much farther a target of the wrong sort looks. */
function misfit(suits: i32, vehicle: bool): i32 {
  if (suits == FOOT && vehicle) return 20;
  if (suits == FOOT_ONLY && vehicle) return 120;
  if (suits == ARMOR && !vehicle) return 16;
  if (suits == ARMOR_ONLY && !vehicle) return 60;
  return 0;
}

// --------------------------------------------------------------- Ground

// The eight ways out of camp, named as the screen shows it; `steps` to
// where the front rank appears, about 16 cells.
const SIDE_DX: StaticArray<i32> = [0, 1, 0, -1, 1, 1, -1, -1];
const SIDE_DY: StaticArray<i32> = [1, 0, -1, 0, 1, -1, 1, -1];
const SIDE_STEPS: StaticArray<i32> = [16, 16, 16, 16, 12, 12, 12, 12];
const SIDES: i32 = 8;
/** Ranks behind the front, then ahead of it where the map ends. */
const DEPTHS: StaticArray<i32> = [0, 1, 2, 3, 4, -1, -2, -3, -4, 5, 6];
const LATERALS: StaticArray<i32> = [0, -1, 1, -2, 2, -3, 3, -4, 4, -5, 5];
/** Cells tried per battle for each of the two passes of `spawn`. */
const ATTEMPTS: i32 = 400;

// Cell diagonals kept clear of the map's edges (LocalSize leaves about so
// many out of sight).
const EDGE_TOP: i32 = 10;
const EDGE_BOTTOM: i32 = 14;
const EDGE_SIDE: i32 = 5;
/** The ground known around the camp, this far either way. */
const HALF: i32 = 20;
const SPAN: i32 = 2 * HALF + 1;
/**
 * Bit `x` of row `y`: a vehicle gets from the camp to that cell over
 * terrain. Trees, walls and units are met by `CreateObject`'s refusals.
 */
const REACHED = new StaticArray<i64>(SPAN);
let surveyed: i64 = NO_HANDLE;
let frontAt: i64 = 0;

// ---------------------------------------------------------------- Start

const LINEUP = new StaticArray<string>(MAX_RAIDERS);
let lineupLen: i32 = 0;

/** The wave's types, unit by unit, into `LINEUP`. */
function lineUp(wave: waves.Wave): void {
  lineupLen = 0;
  for (let at = 0; at < wave.squads.length; at++) {
    const squad = wave.squads[at];
    for (let n = 0; n < squad.count && lineupLen < MAX_RAIDERS; n++) {
      LINEUP[lineupLen] = squad.id;
      lineupLen++;
    }
  }
}

/** Creates the wave's raiders, starts the countdown and sends both sides in. */
export function start(): void {
  const now = frame();
  const wave = waves.get(state.wave);
  for (let at = 0; at < MAX_RAIDERS; at++) state.raiders[at] = NO_HANDLE;
  for (let at = 0; at < FIGHTERS; at++) STUCK[at] = 0;
  const side = spawn(wave);
  let fielded = 0;
  let champions = 0;
  for (let at = 0; at < MAX_COMPANY; at++) {
    const unit = state.unitOf(at);
    if (unit != NO_HANDLE && !state.isEmpty(at) && world.alive(unit)) {
      const kind = state.kindOf(at);
      const id = kind < content.MERCS.length ? content.MERCS[kind].id : "";
      enlist(at, unit, id);
      fielded++;
      if (state.isChampion(at)) champions++;
    } else {
      state.memberUnit[at] = NO_HANDLE;
    }
  }
  perks.battleStarted();

  const arrived = arrivedCount;
  if (arrived == 0) {
    // Nowhere to stand on this map: the wave counts as beaten, or the
    // same wave would come back until the banners were gone.
    log("袭击者找不到进攻营地的路，此波不战而胜。");
    stage = WON;
    until = <i64>(now + AFTERMATH);
    return;
  }
  for (let slot = 0; slot < FIGHTERS; slot++) {
    const unit = fighter(slot);
    if (unit != NO_HANDLE) {
      // They lapse by themselves, so nothing is left to release.
      Modifier.multiply(unit, Prop.SpeedMultiplier, 0, FACE_OFF);
      Modifier.overrideWith(unit, Prop.DisableWeapons, 1, FACE_OFF);
    }
  }
  const alone = fielded == 1 && champions == 1;
  if (alone) {
    for (let at = 0; at < MAX_RAIDERS; at++) {
      const unit = state.raiders[at];
      if (unit != NO_HANDLE) {
        Modifier.multiply(unit, Prop.FirepowerMultiplier, SURROUND, -1);
        Modifier.multiply(unit, Prop.ArmorMultiplier, SURROUND, -1);
      }
    }
  }
  stage = FACING_OFF;
  until = <i64>(now + FACE_OFF);
  countDown(FACE_OFF / 15);
  say(new Line()
    .text("【")
    .text(wave.name)
    .text("】")
    .num(arrived)
    .text("名袭击者自")
    .text(FRONT_NAMES[side])
    .text("来，")
    .num(<i64>(FACE_OFF / 15))
    .text("秒后交战。")
    .text(alone ? "精锐孤军被围，敌攻防×6。" : ""));
}

/**
 * Every frame of a battle; the report (`REPORT_WON`, `REPORT_LOST`, with
 * `reportKills`) once it is decided, else `UNDECIDED`. By then the field is
 * clear of both sides and the timer stands.
 */
export function tick(now: i32): i32 {
  const over = <i64>now >= until;
  if (stage == FACING_OFF) {
    if (!over) return UNDECIDED;
    stage = FIGHTING;
    until = <i64>(now + FIGHT_SECONDS * 15);
    countDown(FIGHT_SECONDS);
    hitAt = <i64>now;
    lookAt = <i64>now;
    look(true);
    look(false);
  } else if (stage == FIGHTING) {
    if (now % COUNT == 0 || over) {
      countLiving();
      if (livingHealth != fieldHealth) {
        fieldHealth = livingHealth;
        hitAt = <i64>now;
      } else if (<i64>now - hitAt >= <i64>STANDOFF) {
        hitAt = <i64>now;
        lookAt = <i64>(now + CLOSING);
        closeIn();
      }
      // Both gone is a loss, as is a raider left when time is up.
      if (livingOurs == 0 || livingTheirs == 0) {
        stage = livingOurs > 0 ? WON : LOST;
        until = <i64>(now + AFTERMATH);
        Action.timerStop();
        return UNDECIDED;
      }
      if (over) {
        log("时间到，袭击者仍在场上。");
        return close(false);
      }
    }
    if (<i64>now < lookAt) return UNDECIDED;
    const phase = now % LOOK;
    if (phase == 0) look(true);
    else if (phase == 7) look(false);
  } else if ((stage == WON || stage == LOST) && over) {
    return close(stage == WON);
  }
  return UNDECIDED;
}

function close(won: bool): i32 {
  countLiving();
  reportKills = max<i64>(arrivedCount - livingTheirs, 0);
  Action.timerStop();
  world.clearForces(state.player());
  world.clearForces(state.enemy());
  for (let at = 0; at < MAX_COMPANY; at++) state.memberUnit[at] = NO_HANDLE;
  for (let at = 0; at < MAX_RAIDERS; at++) state.raiders[at] = NO_HANDLE;
  arrivedCount = 0;
  stage = IDLE;
  return won ? REPORT_WON : REPORT_LOST;
}

function countDown(seconds: i32): void {
  Action.timerSet(seconds);
  Action.timerStart();
}

let livingOurs: i64 = 0;
let livingTheirs: i64 = 0;
let livingHealth: i64 = 0;

/** Forgets the dead; counts the living of both sides and their health. */
function countLiving(): void {
  let ours: i64 = 0;
  let theirs: i64 = 0;
  let health: i64 = 0;
  for (let slot = 0; slot < FIGHTERS; slot++) {
    const unit = fighter(slot);
    if (unit == NO_HANDLE) continue;
    // A unit that no longer answers is gone.
    let left = get(unit, Prop.Health);
    if (lastStatus() != Status.Ok) left = 0;
    const living = left > 0;
    health += max<i64>(left, 0);
    if (slot < MAX_COMPANY) {
      if (living) ours++;
      else state.memberUnit[slot] = NO_HANDLE;
    } else if (living) {
      theirs++;
    } else {
      state.raiders[slot - MAX_COMPANY] = NO_HANDLE;
    }
  }
  livingOurs = ours;
  livingTheirs = theirs;
  livingHealth = health;
}

/**
 * Moves every unit toward the middle of the other side: in reach is not
 * always able to hit, over a cliff or behind a building. The next looks
 * send them at their targets again.
 */
function closeIn(): void {
  const sumX = new StaticArray<i32>(2);
  const sumY = new StaticArray<i32>(2);
  const counts = new StaticArray<i32>(2);
  for (let slot = 0; slot < FIGHTERS; slot++) {
    const unit = fighter(slot);
    if (unit == NO_HANDLE) continue;
    const cell = world.cellOf(unit);
    if (cell == NO_HANDLE) continue;
    const side = slot >= MAX_COMPANY ? 1 : 0;
    sumX[side] += Cell.x(cell);
    sumY[side] += Cell.y(cell);
    counts[side] += 1;
  }
  for (let slot = 0; slot < FIGHTERS; slot++) {
    const unit = fighter(slot);
    if (unit == NO_HANDLE) continue;
    const side = slot < MAX_COMPANY ? 1 : 0;
    if (counts[side] > 0) {
      const x = sumX[side] / counts[side];
      const y = sumY[side] / counts[side];
      Action.setMission(unit, Mission.Move, Cell.at(x, y));
    }
  }
}

/** Fighter `slot`'s unit, the company first; `NO_HANDLE` for none. */
function fighter(slot: i32): Techno {
  if (slot < MAX_COMPANY) return state.memberUnit[slot];
  const at = slot - MAX_COMPANY;
  return at < MAX_RAIDERS ? state.raiders[at] : NO_HANDLE;
}

function enlist(slot: i32, unit: Techno, id: string): void {
  let kind = 0;
  for (let at = 0; at < NATURE_ID.length; at++) {
    if (NATURE_ID[at] == id) {
      kind = at;
      break;
    }
  }
  NATURE[slot] = kind;
  const category = get(unit, Prop.Category);
  IS_VEHICLE[slot] = lastStatus() == Status.Ok && category == 1 ? 1 : 0;
}

const FOE_UNIT = new StaticArray<i64>(MAX_RAIDERS);
const FOE_X = new StaticArray<i32>(MAX_RAIDERS);
const FOE_Y = new StaticArray<i32>(MAX_RAIDERS);
const FOE_VEHICLE = new StaticArray<bool>(MAX_RAIDERS);
/** How many of the looking side are on it. */
const FOE_TAKEN = new StaticArray<i32>(MAX_RAIDERS);
/** Each fighter's present target as an index into the foes, −1 for none. */
const PRESENT = new StaticArray<i32>(FIGHTERS);

/**
 * Sees that each unit of one side fights what it best can: the nearest
 * foe, farther for a misfit or a crowded one.
 */
function look(company: bool): void {
  const ownFrom = company ? 0 : MAX_COMPANY;
  const ownTo = company ? MAX_COMPANY : FIGHTERS;
  const otherFrom = company ? MAX_COMPANY : 0;
  const otherTo = company ? FIGHTERS : MAX_COMPANY;
  let count = 0;
  for (let slot = otherFrom; slot < otherTo; slot++) {
    const unit = fighter(slot);
    if (unit == NO_HANDLE) continue;
    const cell = world.cellOf(unit);
    if (cell == NO_HANDLE) continue;
    if (count < MAX_RAIDERS) {
      FOE_UNIT[count] = unit;
      FOE_X[count] = Cell.x(cell);
      FOE_Y[count] = Cell.y(cell);
      FOE_VEHICLE[count] = IS_VEHICLE[slot] == 1;
      FOE_TAKEN[count] = 0;
      count++;
    }
  }
  if (count == 0) return;
  // Everyone's present target first, so that crowds are known.
  for (let slot = 0; slot < FIGHTERS; slot++) PRESENT[slot] = -1;
  for (let slot = ownFrom; slot < ownTo; slot++) {
    const unit = fighter(slot);
    if (unit == NO_HANDLE) continue;
    let target = get(unit, Prop.Target);
    if (lastStatus() != Status.Ok) target = NO_HANDLE;
    let at = -1;
    for (let foe = 0; foe < count; foe++) {
      if (FOE_UNIT[foe] == target) {
        at = foe;
        break;
      }
    }
    if (at >= 0) FOE_TAKEN[at] += 1;
    PRESENT[slot] = at;
  }
  for (let slot = ownFrom; slot < ownTo; slot++) {
    const unit = fighter(slot);
    if (unit == NO_HANDLE) continue;
    const cell = world.cellOf(unit);
    if (cell == NO_HANDLE) continue;
    const x = Cell.x(cell);
    const y = Cell.y(cell);
    const kind = <i32>min<i64>(NATURE[slot], <i64>(NATURE_ID.length - 1));
    const suits = NATURE_SUITS[kind];
    const present = PRESENT[slot];
    let stuck: i64 = 0;
    if (present >= 0) {
      // Out of reach and not under way: a lost order or no way there.
      if (halfCells(x - FOE_X[present], y - FOE_Y[present]) > 2 * NATURE_REACH[kind] + 3) {
        const moving = get(unit, Prop.Moving);
        if (lastStatus() == Status.Ok && moving == 0) stuck = STUCK[slot] + 1;
      }
    }
    STUCK[slot] = stuck;
    let best = -1;
    let least: i32 = 0;
    let kept: i32 = i32.MAX_VALUE;
    for (let at = 0; at < count; at++) {
      const mine = present == at;
      let score = halfCells(x - FOE_X[at], y - FOE_Y[at]) + misfit(suits, FOE_VEHICLE[at]) +
        CROWDED * (FOE_TAKEN[at] - (mine ? 1 : 0));
      if (mine) {
        if (stuck >= 2) score += BLOCKED;
        kept = score;
      }
      if (best < 0 || score < least) {
        best = at;
        least = score;
      }
    }
    if (best < 0) continue;
    let chosen = best;
    if (present >= 0) {
      if (kept <= least + CHANGE_OF_MIND) {
        if (stuck == 0) continue;
        chosen = present; // The same order once more.
      }
      FOE_TAKEN[present] -= 1;
    }
    FOE_TAKEN[chosen] += 1;
    // A module's own order does not ask its own `CommandAllowed`.
    Action.setMissionOn(unit, Mission.Attack, FOE_UNIT[chosen]);
  }
}

/** A distance in half cells, close to the straight line. */
function halfCells(dx: i32, dy: i32): i32 {
  const ax = abs(dx);
  const ay = abs(dy);
  return 2 * max(ax, ay) + min(ax, ay);
}

// --------------------------------------------------------------- Ground

/** The side the raiders will come from, for the muster's line. */
export function frontName(): string {
  return FRONT_NAMES[front()];
}

/** `front`'s index in `SIDES` and `FRONT_NAMES`, for the interface. */
export function frontIndex(): i64 {
  front();
  return frontAt;
}

/** `standing`'s counts: the company standing, the raiders standing, the raiders that came. */
export let standingOurs: i64 = 0;
export let standingTheirs: i64 = 0;
export let standingArrived: i64 = 0;

/**
 * The company standing, the raiders standing and the raiders that came,
 * for the interface, into `standing*`. Reads only: `countLiving` forgets
 * the dead.
 */
export function standing(): void {
  standingOurs = 0;
  standingTheirs = 0;
  standingArrived = 0;
  if (stage == IDLE) return;
  for (let slot = 0; slot < FIGHTERS; slot++) {
    const unit = fighter(slot);
    if (unit == NO_HANDLE) continue;
    const health = get(unit, Prop.Health);
    if (lastStatus() == Status.Ok && health > 0) {
      if (slot < MAX_COMPANY) standingOurs++;
      else standingTheirs++;
    }
  }
  standingArrived = arrivedCount;
}

/** The index of the side the raiders come from. */
function front(): i32 {
  const camp = state.camp();
  if (surveyed != camp) {
    surveyed = camp;
    survey(camp);
    let bestAt = 0;
    let bestOpen = -1;
    for (let at = 0; at < SIDES; at++) {
      const open = openness(at);
      if (open > bestOpen) {
        bestAt = at;
        bestOpen = open;
      }
    }
    frontAt = bestAt;
  }
  return <i32>min<i64>(frontAt, <i64>(SIDES - 1));
}

const OPEN_ROWS = new StaticArray<u64>(SPAN);
const FLOOD = new StaticArray<u64>(SPAN);

/** Reads the ground around the camp and floods it from there. */
function survey(camp: Cell): void {
  const west = Cell.x(camp) - HALF;
  const north = Cell.y(camp) - HALF;
  const corner = Cell.at(max(west, 0), max(north, 0));
  inside(camp, west, north);
  // The closed cells are few, and each one found costs operations.
  const blocked = Find.cells()
    .within(corner, SPAN, SPAN)
    .where(SpeedType.passable(SpeedType.Track), Op.Eq, 0);
  ruleOut(blocked, west, north);
  for (let y = 0; y < SPAN; y++) FLOOD[y] = 0;
  // From the open cell nearest the camp.
  let seeded = false;
  for (let ring = 0; ring <= 4 && !seeded; ring++) {
    for (let dy = -ring; dy <= ring && !seeded; dy++) {
      for (let dx = -ring; dx <= ring; dx++) {
        const x = HALF + dx;
        const y = HALF + dy;
        if (((OPEN_ROWS[y] >> <u64>x) & 1) == 1) {
          FLOOD[y] = (<u64>1) << <u64>x;
          seeded = true;
          break;
        }
      }
    }
  }
  while (true) {
    let grown = false;
    // Down, then up: few rounds even round a bend.
    for (let step = 0; step < 2 * SPAN; step++) {
      const y = step < SPAN ? step : 2 * SPAN - 1 - step;
      const before = FLOOD[y];
      let row = before;
      if (y > 0) row |= FLOOD[y - 1];
      if (y + 1 < SPAN) row |= FLOOD[y + 1];
      row &= OPEN_ROWS[y];
      while (true) {
        const wider = (row | (row << 1) | (row >> 1)) & OPEN_ROWS[y];
        if (wider == row) break;
        row = wider;
      }
      if (row != before) {
        FLOOD[y] = row;
        grown = true;
      }
    }
    if (!grown) break;
  }
  for (let y = 0; y < SPAN; y++) REACHED[y] = <i64>FLOOD[y];
}

function reached(dx: i32, dy: i32): bool {
  const x = dx + HALF;
  const y = dy + HALF;
  if (x < 0 || x >= SPAN || y < 0 || y >= SPAN) return false;
  return ((<u64>REACHED[y] >> <u64>x) & 1) == 1;
}

/** Steps from (`x`, `y`) along (`dx`, `dy`) that stay on the map, up to 31. */
function stepsToEdge(x: i32, y: i32, dx: i32, dy: i32): i32 {
  let onMap = 0;
  let beyond = 32;
  while (beyond - onMap > 1) {
    const step = (onMap + beyond) / 2;
    const sx = x + dx * step;
    const sy = y + dy * step;
    let found = false;
    if (sx >= 0 && sy >= 0) {
      get(Cell.at(sx, sy), Prop.LandType);
      found = lastStatus() == Status.Ok;
    }
    if (found) onMap = step;
    else beyond = step;
  }
  return onMap;
}

/**
 * The cells within the map into `OPEN_ROWS`, less the rows along its edge
 * that the player does not see. The map is a diamond: each edge is found
 * straight across it from the camp, in a few reads.
 */
function inside(camp: Cell, west: i32, north: i32): void {
  const x = Cell.x(camp);
  const y = Cell.y(camp);
  // The screen's top, bottom, left and right, in cells each a diagonal.
  const top = x + y - max(2 * stepsToEdge(x, y, -1, -1) - EDGE_TOP, 0);
  const bottom = x + y + max(2 * stepsToEdge(x, y, 1, 1) - EDGE_BOTTOM, 0);
  const left = x - y - max(2 * stepsToEdge(x, y, -1, 1) - EDGE_SIDE, 0);
  const right = x - y + max(2 * stepsToEdge(x, y, 1, -1) - EDGE_SIDE, 0);
  for (let at = 0; at < SPAN; at++) {
    const row = north + at;
    const first = max(max(max(top - row, left + row) - west, -west), 0);
    const last = min(min(bottom - row, right + row) - west, SPAN - 1);
    OPEN_ROWS[at] = 0;
    if (row >= 0 && first <= last) {
      OPEN_ROWS[at] = (((<u64>1) << <u64>(last + 1)) - 1) & ~(((<u64>1) << <u64>first) - 1);
    }
  }
}

const FOUND_CELLS = new StaticArray<i64>(256);

/** Clears the cells `query` finds. */
function ruleOut(query: Find, west: i32, north: i32): void {
  let after: i64 = 0;
  while (true) {
    const page = query.page(FOUND_CELLS, after);
    if (lastStatus() != Status.Ok) break;
    const count = page.count;
    const next = page.next;
    for (let at = 0; at < count && at < FOUND_CELLS.length; at++) {
      const cell = FOUND_CELLS[at];
      const row = Cell.y(cell) - north;
      if (row >= 0 && row < SPAN) {
        const bit = <u64>(Cell.x(cell) - west) & 63;
        OPEN_ROWS[row] &= ~((<u64>1) << bit);
      }
    }
    if (next == 0) break;
    after = next;
  }
}

// `along`: from the camp, `out` steps toward side `side` and `across` to the right of it.
function alongX(side: i32, out: i32, across: i32): i32 {
  return SIDE_DX[side] * out - SIDE_DY[side] * across;
}
function alongY(side: i32, out: i32, across: i32): i32 {
  return SIDE_DY[side] * out + SIDE_DX[side] * across;
}

const ACROSS_FRONT: StaticArray<i32> = [-4, -2, 0, 2, 4];
const BACK_FRONT: StaticArray<i32> = [0, 2, 4];
const ACROSS_WAY: StaticArray<i32> = [-2, 0, 2];

/** How open the ground is where a wave would stand and on its way in. */
function openness(side: i32): i32 {
  const steps = SIDE_STEPS[side];
  let open = 0;
  // A sample of the cells: this runs for every side in one frame.
  for (let a = 0; a < ACROSS_FRONT.length; a++) {
    for (let b = 0; b < BACK_FRONT.length; b++) {
      const out = steps + BACK_FRONT[b];
      if (reached(alongX(side, out, ACROSS_FRONT[a]), alongY(side, out, ACROSS_FRONT[a]))) open += 6;
    }
  }
  for (let a = 0; a < ACROSS_WAY.length; a++) {
    // As many samples for a diagonal as for a straight way in.
    for (let part = 1; part <= 6; part++) {
      const out = (steps * part) / 6;
      if (reached(alongX(side, out, ACROSS_WAY[a]), alongY(side, out, ACROSS_WAY[a]))) open += 4;
    }
  }
  return open;
}

/** `spawn`'s progress through `LINEUP`. */
let nextRaider: i32 = 0;
let attempts: i32 = 0;

/**
 * Tries to place the next raider at camp + (`dx`, `dy`). False when the
 * attempts are used up (nothing was tried).
 */
function tryPlace(dx: i32, dy: i32): bool {
  if (attempts >= ATTEMPTS) return false;
  attempts++;
  const id = LINEUP[nextRaider];
  const ty = Rules.technoType(id);
  if (ty == NO_HANDLE) {
    nextRaider++;
    return true;
  }
  const unit = Action.createObject(world.offset(state.camp(), dx, dy), ty, state.enemy());
  if (lastStatus() == Status.Ok) {
    const arrived = <i32>arrivedCount;
    state.raiders[arrived] = unit;
    enlist(MAX_COMPANY + arrived, unit, id);
    arrivedCount = arrived + 1;
    nextRaider++;
  }
  return true;
}

const TRIED = new StaticArray<bool>(SIDES);

/**
 * Places the wave in ranks on the most open side. What does not fit goes
 * to the next most open one; failing all, to any cell away from the camp.
 * Returns the side most of it came from.
 */
function spawn(wave: waves.Wave): i32 {
  const first = front();
  for (let at = 0; at < SIDES; at++) TRIED[at] = false;
  lineUp(wave);
  nextRaider = 0;
  attempts = 0;
  arrivedCount = 0;
  let side = first;
  let mainCount: i64 = 0;
  let mainSide = first;
  let spent = false;
  for (let round = 0; round < SIDES && !spent; round++) {
    TRIED[side] = true;
    const before = arrivedCount;
    const steps = SIDE_STEPS[side];
    let done = false;
    for (let d = 0; d < DEPTHS.length && !done && !spent; d++) {
      for (let l = 0; l < LATERALS.length; l++) {
        if (nextRaider >= lineupLen) {
          done = true;
          break;
        }
        const out = steps + DEPTHS[d];
        const dx = alongX(side, out, LATERALS[l]);
        const dy = alongY(side, out, LATERALS[l]);
        if (!reached(dx, dy)) continue;
        if (!tryPlace(dx, dy)) {
          spent = true;
          break;
        }
      }
    }
    if (spent) break;
    if (arrivedCount - before > mainCount) {
      mainCount = arrivedCount - before;
      mainSide = side;
    }
    if (nextRaider >= lineupLen) break;
    let nextSide = -1;
    let most = 0;
    for (let at = 0; at < SIDES; at++) {
      if (!TRIED[at]) {
        const open = openness(at);
        if (nextSide < 0 || open > most) {
          nextSide = at;
          most = open;
        }
      }
    }
    if (nextSide < 0) break;
    side = nextSide;
  }

  if (arrivedCount == 0) {
    // No reachable ground: rings around the camp, the farthest first.
    nextRaider = 0;
    attempts = 0;
    let stop = false;
    for (let ring = 16; ring >= 4 && !stop; ring--) {
      for (let step = 0; step < 8 * ring; step++) {
        if (nextRaider >= lineupLen) {
          stop = true;
          break;
        }
        const alongEdge = (step % (2 * ring)) - ring;
        const edge = step / (2 * ring);
        let dx: i32;
        let dy: i32;
        if (edge == 0) {
          dx = alongEdge;
          dy = -ring;
        } else if (edge == 1) {
          dx = ring;
          dy = alongEdge;
        } else if (edge == 2) {
          dx = -alongEdge;
          dy = ring;
        } else {
          dx = -ring;
          dy = -alongEdge;
        }
        if (!tryPlace(dx, dy)) {
          stop = true;
          break;
        }
      }
    }
  }
  return mainSide;
}
