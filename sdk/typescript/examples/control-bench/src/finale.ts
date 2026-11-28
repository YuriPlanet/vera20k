// The bench's finale (see `index.ts`), the port of the Rust `finale.rs`:
// run after the areas, since its points pause or end the match the areas
// play in. Marks are named `finale.<point>`; `bench.all` follows the last.
//
// In order:
// 1. `SuspendProduction`: the finale claims the computer house's
//    `Production` domain, so its AI starts nothing more and its builds come
//    from house orders. It queues a type (a conscript, else a Rhino, else a
//    power plant) the AI is not building, waits until that build is under
//    way (`Progress` between 1 and 90), and orders `SuspendProduction`;
//    `Progress` stands still. `QueueProduction` of the same type resumes
//    it. The claim goes before the house's objects do.
// 2. `Defeat`: every object of the computer house is removed. Its defeat
//    gate then asks; the first answer vetoes (`defeat-veto`), the next
//    allows (`defeat`: the house reads `IsDefeated`).
// 3. `EndFor` the player as the winner (`end`); `after-end` once the module
//    has run 30 more frames.
//
// Each step gives up after its limit with a `BENCH-FAIL`, so the steps take
// at most about 1050 frames.
import {
  Action, Category, Command, DefeatDecision, Event, Find, NO_HANDLE, Op, Order, Outcome, Prop, Status,
  Techno, Trigger, House, frame, get, lastStatus, set, Rules,
} from "@vera20k/sdk";
import { benchFail, enemy, mark, player } from "./bench";

/** Every mark of the finale. */
export const MARKS: StaticArray<string> = [
  "finale.suspend",
  "finale.resume",
  "finale.defeat-veto",
  "finale.defeat",
  "finale.end",
  "finale.after-end",
];

// Stages.
const PICK: i64 = 1;
const SUSPENDED: i64 = 2;
const RESUMED: i64 = 3;
const REMOVE: i64 = 4;
const DEFEAT: i64 = 5;
const END: i64 = 6;
const AFTER: i64 = 7;
const DONE: i64 = 8;

/** Frames production is watched after an order. */
const WATCH: i64 = 45;
/** Frames each candidate type gets to be under way. */
const QUEUE_LIMIT: i64 = 90;
const REMOVE_LIMIT: i64 = 150;
const DEFEAT_LIMIT: i64 = 450;
/** Frames the house must stay undefeated after the veto. */
const VETO_HOLD: i64 = 10;

/** The `Production` bit of `ControlDomains` (RFC 0001 6i). */
const DOMAIN_PRODUCTION: i64 = 1;

let stageNow: i64 = 0;
/** The frame the stage began. */
let stageAt: i64 = 0;
/** The watched factory (index into the factory tables), the type it builds and its last `Progress` reading. */
let factoryIndex: i32 = 0;
let watchedType: i64 = 0;
let lastProgress: i64 = -1;
/** The candidate queued now, and whether the finale set the `Production` claim. */
let candidate: i32 = 0;
let claimed: bool = false;
/** The frame of the first veto, the gate's asks since, and whether the next ask is allowed. */
let vetoedAt: i64 = 0;
let asks: i64 = 0;
let allow: bool = false;

/** The types the finale queues, with their factory: Soviet, which the computer house (Russia) builds. */
const CANDIDATE_NAMES: StaticArray<string> = ["E2", "HTNK", "NAPOWR"];
const CANDIDATE_FACTORIES: StaticArray<i32> = [0, 1, 2];

/** The computer house's factories: `Type`, `Progress` and `Ready` of each kind. */
const FACTORY_TYPE: StaticArray<i32> = [
  Prop.Factory_Infantry_Type, Prop.Factory_Vehicle_Type, Prop.Factory_Building_Type, Prop.Factory_Defense_Type,
];
const FACTORY_PROGRESS: StaticArray<i32> = [
  Prop.Factory_Infantry_Progress, Prop.Factory_Vehicle_Progress, Prop.Factory_Building_Progress,
  Prop.Factory_Defense_Progress,
];
const FACTORY_READY: StaticArray<i32> = [
  Prop.Factory_Infantry_Ready, Prop.Factory_Vehicle_Ready, Prop.Factory_Building_Ready, Prop.Factory_Defense_Ready,
];

const NO_UNITS = new StaticArray<Techno>(0);
const PAGE = new StaticArray<i64>(32);

function stage(next: i64, now: i64): void {
  stageNow = next;
  stageAt = now;
}

/** Whether the finale began (the areas' frame work, records and decisions stop). */
export function finaleStarted(): bool {
  return stageNow != 0;
}

/** Whether every step ran. */
export function finaleDone(): bool {
  return stageNow == DONE;
}

/** A reading of the computer house, 0 when it cannot be read. */
function read(prop: Prop): i64 {
  const v = get(enemy(), prop);
  return lastStatus() == Status.Ok ? v : 0;
}

/**
 * Starts the finale on frame `now`: the computer house can pay for what it
 * builds, its `Production` is claimed, and the `Defeat` decision is made
 * before its objects go.
 */
export function begin(now: i32): void {
  if (finaleStarted()) return;
  stage(PICK, <i64>now);
  const foe = enemy();
  Action.addMoney(foe, 10000);
  const domains = read(Prop.ControlDomains);
  if ((domains & DOMAIN_PRODUCTION) == 0) {
    if (set(foe, Prop.ControlDomains, domains | DOMAIN_PRODUCTION) == Status.Ok) claimed = true;
    else benchFail("finale.suspend", "the Production claim was not taken");
  }
  if (Trigger.on(Event.Defeat).decideSync(defeatGate) == NO_HANDLE) {
    benchFail("finale.defeat", "the Defeat trigger could not be made");
  }
}

/** Every frame once the finale began. */
export function finaleFrame(at: i32): void {
  const now = <i64>at;
  const since = now - stageAt;
  const s = stageNow;
  if (s == PICK) pick(now, since);
  else if (s == SUSPENDED) suspended(now, since);
  else if (s == RESUMED) resumed(now, since);
  else if (s == REMOVE) removeAll(now, since);
  else if (s == DEFEAT) defeat(now, since);
  else if (s == END) {
    if (Action.endFor(player(), Outcome.Win) == Status.Ok) {
      mark("finale.end");
      stage(AFTER, now);
    } else {
      benchFail("finale.end", "EndFor answered an error");
      stage(DONE, now);
    }
  } else if (s == AFTER && since >= 30) {
    mark("finale.after-end");
    stage(DONE, now);
  }
}

function nextCandidate(now: i64): void {
  candidate++;
  stage(PICK, now);
}

/**
 * Each candidate in turn: queued on the stage's first frame (skipped while
 * the AI's own factory builds that type), then watched every 3 frames
 * until its build is under way, when `SuspendProduction` goes out. A
 * candidate not under way after `QUEUE_LIMIT` frames gives way to the next.
 */
function pick(now: i64, since: i64): void {
  const index = candidate;
  if (index >= CANDIDATE_NAMES.length) {
    benchFail("finale.suspend", "no candidate type got under way");
    stage(REMOVE, now);
    return;
  }
  const kind = Rules.technoType(CANDIDATE_NAMES[index]);
  if (kind == NO_HANDLE) {
    nextCandidate(now);
    return;
  }
  const f = CANDIDATE_FACTORIES[index];
  const ty = FACTORY_TYPE[f];
  const progress = FACTORY_PROGRESS[f];
  if (since == 1) {
    if (read(ty) == kind || House.order(enemy(), Order.queueProduction(kind)) != Status.Ok) {
      nextCandidate(now);
    }
    return;
  }
  if (since > QUEUE_LIMIT) {
    nextCandidate(now);
    return;
  }
  if (since < 4 || since % 3 != 0 || read(ty) != kind) return;
  const p = read(progress);
  if (p < 1 || p > 90) return;
  const order = new Order(Command.SuspendProduction, 0, kind, NO_UNITS);
  if (House.order(enemy(), order) != Status.Ok) {
    benchFail("finale.suspend", "the order was turned down at once");
    stage(REMOVE, now);
    return;
  }
  factoryIndex = f;
  watchedType = kind;
  lastProgress = -1;
  stage(SUSPENDED, now);
}

/**
 * The order ran the frame after it was given: `Progress` is read then, and
 * must be the same `WATCH` frames later. Then the same type is queued
 * again, which resumes it.
 */
function suspended(now: i64, since: i64): void {
  const progress = FACTORY_PROGRESS[factoryIndex];
  if (since == 3) lastProgress = read(progress);
  if (since < WATCH) return;
  const held = read(FACTORY_TYPE[factoryIndex]) == watchedType && read(progress) == lastProgress;
  if (!held) {
    benchFail("finale.suspend", "the factory went on building");
    stage(REMOVE, now);
    return;
  }
  mark("finale.suspend");
  if (House.order(enemy(), Order.queueProduction(watchedType)) != Status.Ok) {
    benchFail("finale.resume", "the order was turned down at once");
    stage(REMOVE, now);
    return;
  }
  stage(RESUMED, now);
}

/** The resumed factory moved on: further, finished, or done with the type. */
function resumed(now: i64, since: i64): void {
  if (since < WATCH) return;
  const moved = read(FACTORY_PROGRESS[factoryIndex]) > lastProgress
    || read(FACTORY_READY[factoryIndex]) != 0
    || read(FACTORY_TYPE[factoryIndex]) != watchedType;
  if (moved) mark("finale.resume");
  else benchFail("finale.resume", "the factory stood still");
  stage(REMOVE, now);
}

/** Removes the computer house's units and buildings, a page a frame, until none is left. */
function removeAll(now: i64, since: i64): void {
  // The claim goes first: the house is the AI's again until it is gone.
  if (claimed) {
    claimed = false;
    const domains = read(Prop.ControlDomains);
    set(enemy(), Prop.ControlDomains, domains & ~DOMAIN_PRODUCTION);
  }
  // Units and buildings: `remove` takes no projectile.
  const find = Find.objects().ownedBy(enemy()).where(Prop.Category, Op.Lt, <i64>Category.Projectile);
  // A page can be empty with more to examine after it (`next`).
  let after: i64 = 0;
  let count = 0;
  while (true) {
    const page = find.page(PAGE, after);
    if (lastStatus() != Status.Ok) {
      count = 0;
      break;
    }
    if (page.count == 0 && page.next != 0) {
      after = page.next;
      continue;
    }
    count = page.count;
    break;
  }
  for (let i = 0; i < count; i++) Action.remove(PAGE[i]);
  if (count == 0 || since > REMOVE_LIMIT) stage(DEFEAT, now);
}

/** Waits for the gate's veto to hold, then for the allowed defeat. */
function defeat(now: i64, since: i64): void {
  const defeated = get(enemy(), Prop.IsDefeated) == 1 && lastStatus() == Status.Ok;
  if (!allow) {
    if (vetoedAt != 0 && now >= vetoedAt + VETO_HOLD && asks >= 2) {
      if (defeated) benchFail("finale.defeat-veto", "the house was defeated anyway");
      else mark("finale.defeat-veto");
      allow = true;
    }
  } else if (defeated) {
    mark("finale.defeat");
    stage(END, now);
    return;
  }
  if (since > DEFEAT_LIMIT) {
    benchFail("finale.defeat", "the defeat gate never asked or never defeated");
    stage(END, now);
  }
}

/** The `Defeat` gate for the computer house: vetoed until the veto held, then allowed. */
function defeatGate(d: DefeatDecision): void {
  if (d.record.house != enemy()) return;
  if (allow) {
    d.setResult(1);
    return;
  }
  d.setResult(0);
  asks++;
  if (vetoedAt == 0) vetoedAt = <i64>frame();
}
