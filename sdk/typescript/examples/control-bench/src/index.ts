// "Control Bench", the TypeScript port of the Rust SDK's
// `sdk/examples/control-bench`: one `rules.wasm` that reaches nearly every
// control point the engine offers and logs a line `DEMO-OK <mark>` the
// first time each one works. It is a test fixture, not a game: it spawns
// its own units next to the player's start and drives them itself, so a
// Battle against one computer house with nobody at the controls is enough.
//
// The engine test `control_bench_reaches_every_mark` loads it the way a
// mod loads (`VERA20K_TEST_CONTROL_BENCH`, see `README.zh-CN.md`). It logs
// the same marks and lines as the Rust build.
//
// The work is split by area, one file each, as in the Rust bench:
// - `basics`: host calls, properties, writes, variables, searches,
//   modifiers of every wired stat, INI registration;
// - `triggers`: trigger events and polls, atomic and stock actions, chains,
//   house orders and their results, claims, INI triggers;
// - `combat`: the combat decisions (damage, death, fire, projectiles);
// - `movement`: production, command, movement, path and unit AI decisions;
// - `finale`: once every area is done (or at frame 4800), the points that
//   pause or end the match; the areas' frame work, records and decisions
//   stop then.
//
// This file owns the lifecycle, the progress report and the rules stage
// (`onRules`, marks `rules.*`); `bench.ts` the marks and the arena.
import { Every, NO_HANDLE, Prop, Rules, Status, frame, get, lastStatus, log, onFrame, onRules, onScenarioStart } from "@vera20k/sdk";
import { benchFail, contains, findSides, hash, mark, parse } from "./bench";
import * as basics from "./basics";
import * as triggers from "./triggers";
import * as combat from "./combat";
import * as movement from "./movement";
import { MARKS as FINALE_MARKS, begin, finaleDone, finaleFrame, finaleStarted } from "./finale";

// ------------------------------------------------------------ Rules stage

/**
 * The rules stage's marks: the initialization ran before `onRules`,
 * `Rules.get` reads a retail key, TechnoType names and their properties
 * answer there, and a `Rules.set` of a type's `Cost` is what the match's
 * type reads.
 */
const RULES_MARKS: StaticArray<string> = ["rules.init", "rules.get", "rules.type-lookup", "rules.set"];

/** The type and the cost `onRules` gives it. No one builds the Yuri Gattling tank in the bench's match. */
const SET_TYPE = "YTNK";
const SET_COST: i64 = 1234;
const SET_COST_TEXT = "1234";

/** The initialization ran; `onRules` wrote the cost. */
let initRan: bool = false;
let costSet: bool = false;

function rulesStage(): void {
  if (initRan && frame() == 0) mark("rules.init");
  // The GI's Strength, as text and as the type's property.
  const gi = Rules.technoType("E1");
  const text = parse(Rules.get("E1", "Strength"));
  const textOk = lastStatus() == Status.Ok;
  const strength = get(gi, Prop.Strength);
  if (gi != NO_HANDLE && textOk && text >= 0 && lastStatus() == Status.Ok && strength == text && text > 0) {
    mark("rules.get");
  } else {
    benchFail("rules.get", "[E1] Strength did not read as the type's");
  }
  const kind = Rules.technoType(SET_TYPE);
  if (kind == NO_HANDLE) {
    benchFail("rules.type-lookup", "YTNK is not a TechnoType here");
    return;
  }
  const cost = get(kind, Prop.Cost);
  if (lastStatus() == Status.Ok && cost > 0 && cost != SET_COST) {
    mark("rules.type-lookup");
  } else {
    benchFail("rules.type-lookup", "YTNK's Cost did not read");
    return;
  }
  let written = Rules.set(SET_TYPE, "Cost", SET_COST_TEXT) == Status.Ok;
  if (written) {
    const back = parse(Rules.get(SET_TYPE, "Cost"));
    written = lastStatus() == Status.Ok && back == SET_COST;
  }
  if (written) costSet = true;
  else benchFail("rules.set", "the written Cost did not read back");
}

/** On the arena's frame: the match's type has the cost `onRules` wrote. */
function rulesCheck(): void {
  if (!costSet) return;
  const kind = Rules.technoType(SET_TYPE);
  const cost = get(kind, Prop.Cost);
  if (kind != NO_HANDLE && lastStatus() == Status.Ok && cost == SET_COST) mark("rules.set");
  else benchFail("rules.set", "the match's YTNK has another Cost");
}

// ----------------------------------------------------------------- Report

/** The areas' marks; the finale's come last. */
const AREAS: StaticArray<StaticArray<string>> = [
  RULES_MARKS, basics.MARKS, triggers.MARKS, combat.MARKS, movement.MARKS, FINALE_MARKS,
];

/** Each area's mark hashes, made once here so a report hashes no names. */
const AREA_HASHES: StaticArray<StaticArray<i64>> = areaHashes();

function areaHashes(): StaticArray<StaticArray<i64>> {
  const out = new StaticArray<StaticArray<i64>>(AREAS.length);
  for (let group = 0; group < AREAS.length; group++) {
    const marks = AREAS[group];
    const hashes = new StaticArray<i64>(marks.length);
    for (let i = 0; i < marks.length; i++) hashes[i] = hash(marks[i]);
    out[group] = hashes;
  }
  return out;
}

/** Every `MISSING_EVERY` frames until all are in, each missing mark. */
const MISSING_EVERY: i32 = 1500;
/** Most `BENCH-MISSING` lines a report writes. */
const MISSING_LINES: i32 = 24;
/** The frame (a report frame) the finale begins at even if an area still misses marks. */
const FINALE_DEADLINE: i32 = 4800;

let allDone: bool = false;

/**
 * Every 300 frames until all are in, `BENCH frame <n>: <reached>/<total>`
 * and every `MISSING_EVERY` frames a line `BENCH-MISSING <mark>` for each
 * missing one. Once the match's areas (`basics` to `movement`) have every
 * mark, or at `FINALE_DEADLINE`, the finale begins; once every mark is in,
 * the one mark `bench.all`.
 */
function report(now: i32): void {
  if (allDone) return;
  const deadline = now >= FINALE_DEADLINE && !finaleStarted();
  const listMissing = now % MISSING_EVERY == 0 || deadline;
  let listed = 0;
  let total = 0;
  let reached = 0;
  let areasMissing = 0;
  for (let group = 0; group < AREAS.length; group++) {
    const marks = AREAS[group];
    const hashes = AREA_HASHES[group];
    for (let i = 0; i < marks.length; i++) {
      const name = marks[i];
      total++;
      if (contains(unchecked(hashes[i]))) {
        reached++;
        continue;
      }
      if (group >= 1 && group < AREAS.length - 1) areasMissing++;
      if (listMissing && listed <= MISSING_LINES) {
        log("BENCH-MISSING " + (listed == MISSING_LINES ? "..." : name));
        listed++;
      }
    }
  }
  log("BENCH frame " + now.toString() + ": " + reached.toString() + "/" + total.toString());
  if (reached == total) {
    allDone = true;
    mark("bench.all");
  } else if (areasMissing == 0 || deadline) {
    begin(now);
  }
}

// --------------------------------------------------------------- Lifecycle

/** The frame the arena opens on: the starting units are on the map by the second frame. */
const OPEN_FRAME: i32 = 2;
/** 0 before the arena opened, −1 when it could not, else its frame. */
let opened: i32 = 0;

function tick(): void {
  const now = frame();
  if (opened == 0) {
    if (now < OPEN_FRAME) return;
    if (!findSides()) {
      opened = -1;
      log("BENCH-FAIL arena: needs one human house and a computer opponent");
      return;
    }
    opened = now;
    rulesCheck();
    basics.open();
    triggers.open();
    combat.open();
    movement.open();
    return;
  }
  if (opened < 0) return;
  if (finaleStarted()) {
    finaleFrame(now);
    if (finaleDone()) report(now);
  } else {
    basics.frame(now);
    triggers.frame(now);
    combat.frame(now);
    movement.frame(now);
  }
  if (now % 300 == 0) report(now);
}

onRules(rulesStage);

onScenarioStart((): void => {
  basics.start();
  triggers.start();
  combat.start();
  movement.start();
});

onFrame(new Every(1), tick);

// The initialization (`vera_init`): top-level code.
initRan = true;
basics.init();
triggers.init();
combat.init();
movement.init();
