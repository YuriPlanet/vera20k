// "Meteor Storm", a playtest `rules.wasm` about the map itself, the same as
// the Rust SDK's `sdk/examples/meteor-storm`: cells, timers, randomness and
// the stock trigger actions.
//
// - A `Timer` trigger (`Trigger.every`) delivers a record every 45 seconds;
//   the module answers it in `onStorm`.
// - A cell search (`Find.cells`) with a property clause finds ore cells,
//   starting at a random cell index (`random`, the module's own
//   deterministic stream, the same on every machine).
// - Cell properties (`Cell.ore`, `Cell.height`) and an object search
//   around a cell (`Find.near`).
// - Actions on cells: `spawnAnim` and `detonateWarhead`.
// - Stock trigger actions: the mission timer counts down to the next storm
//   (`timerSet`, `timerStart`, read back with `Global.missionTimer`), and
//   every fourth storm starts a lightning storm (`ionStormStart`,
//   `Global.ionStormActive`).
// - Stock Red Alert 2 global variable 20 counts the storms (`Var.global`).
import {
  Action, Cell, Event, Find, Global, NO_HANDLE, NO_ID, Op, Prop, Rules, Status, TimerEvent,
  Trigger, Var, lastStatus, log, namedAction, onScenarioStart, random,
} from "@vera20k/sdk";

/** Seconds between storms. */
const STORM_SECONDS: i32 = 45;
/** Meteors per storm. */
const METEORS: i32 = 3;
/** Damage at the impact cell. */
const DAMAGE: i32 = 120;
/** Red Alert 2 global variable that counts the storms. */
const STORMS = Var.global(20);
/** Explosion animations to try, in order. */
const ANIMS: StaticArray<string> = ["TWLT070", "S_BANG48", "FIRE01"];

const cells = new StaticArray<i64>(64);
const caughtPage = new StaticArray<i64>(32);

/** The mission timer shows the time to the next storm. */
function restartTimer(): void {
  Action.timerSet(STORM_SECONDS);
  Action.timerStart();
}

/** Ore cells after cell index `after`, written to `cells`; how many. */
function oreCells(after: i64): i32 {
  const page = Find.cells().where(Prop.Ore, Op.Gt, 0).page(cells, after);
  return lastStatus() == Status.Ok ? page.count : 0;
}

/** `value` read through a call that may fail: 0 when it did. */
function orZero(value: i64): i64 {
  return lastStatus() == Status.Ok ? value : 0;
}

function onStorm(e: TimerEvent): void {
  const storm = STORMS.get() + 1;
  STORMS.set(storm);
  const warhead = Rules.warhead("HE");
  if (warhead == NO_HANDLE) {
    log("Meteor Storm: warhead HE not found");
    return;
  }
  let anim = NO_ID;
  for (let i = 0; i < ANIMS.length && anim == NO_ID; i++) anim = Rules.anim(ANIMS[i]);

  // Up to 64 ore cells from a random place on the map; wrap to the start
  // when the random place is past the last ore cell.
  let count = oreCells(<i64>random(0, 60000));
  if (count == 0) count = oreCells(0);
  if (count == 0) {
    log("Meteor Storm: no ore left on the map - the sky stays clear");
    restartTimer();
    return;
  }

  for (let m = 0; m < METEORS; m++) {
    const cell: Cell = cells[random(0, count - 1)];
    const near = Find.objects().near(cell, 2).page(caughtPage, 0);
    const caught = lastStatus() == Status.Ok ? near.count : 0;
    if (anim != NO_ID) Action.spawnAnim(cell, anim);
    Action.detonateWarhead(cell, warhead, DAMAGE);
    const ore = orZero(Cell.ore(cell));
    const height = orZero(Cell.height(cell));
    log("Meteor Storm: impact at " + Cell.x(cell).toString() + "," + Cell.y(cell).toString() +
      " (ore " + ore.toString() + ", height " + height.toString() + ") - " +
      caught.toString() + " units in the blast");
  }

  if (storm % 4 == 0) {
    const active = Global.ionStormActive();
    if (lastStatus() == Status.Ok && !active && Action.ionStormStart(450) == Status.Ok) {
      log("Meteor Storm: a lightning storm follows the meteors!");
    }
  }
  restartTimer();
  const timer = orZero(Global.missionTimer());
  log("Meteor Storm: storm " + storm.toString() + " over; next in " + (timer / 15).toString() + " s");
}

namedAction("OnStorm", onStorm);

onScenarioStart((): void => {
  log("Meteor Storm: every 45 s meteors strike the ore fields; the timer counts down.");
  log("Every 4th storm brings lightning. Keep your harvesters moving!");
  if (Trigger.every(STORM_SECONDS * 15).enqueue(onStorm).done() == NO_HANDLE) {
    log("Meteor Storm: the timer trigger was refused");
  }
  restartTimer();
});
