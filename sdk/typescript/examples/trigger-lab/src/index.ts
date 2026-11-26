// "Trigger Lab", a playtest `rules.wasm`, the same as the Rust SDK's
// `sdk/examples/trigger-lab`: a tour of the trigger system for the human
// player, each trigger announcing itself in the message list.
//
// - `Birth` bound to types, with a subject clause on `Owner`, an immediate
//   action (`ActionDef.log`, run by the engine as the trigger fires) and a
//   module action that counts in a house variable (`Built`, `vera.json`).
// - `Fire` bound to a weapon (the GI's `M60`), `once()`: only the first
//   shot of the match reports.
// - `PassengerAdded` bound to transports: who boarded and the space left.
// - `PropertyPoll` on the house's `Money` (`Trigger.onProperty` with
//   `house`): fires each time the money rises to 5000.
// - `HousePowerPoll`: warns when the house's power runs short.
// - `VariablePoll` on Red Alert 2 local variable 10, which the money
//   trigger sets: one trigger driving another. It also disables the money
//   trigger for a minute (`TriggerRef.setEnabled`).
// - `DistancePoll` bound to one object, the starting MCV: reports once it
//   is 6 cells from where it started.
// - Every minute an army report reads every unit's health in one call
//   (`getMany`).
import {
  ActionDef, BirthEvent, Category, Cell, Event, Every, FireEvent, Find, House, NO_HANDLE, Op,
  PassengerAddedEvent, PollEvent, Prop, Rules, Status, Techno, Trigger, TriggerRef, Var,
  frame, get, getMany, lastStatus, log, namedAction, onFrame, onScenarioStart,
} from "@vera20k/sdk";
import { Built } from "./generated/vars";

const TANKS: StaticArray<string> = ["MTNK", "HTNK", "FV", "APOC", "SREF", "TNKD", "LTNK", "YTNK"];
const TRANSPORTS: StaticArray<string> = ["FV", "BFRT", "LCRF", "SAPC"];
const MCVS: StaticArray<string> = ["AMCV", "SMCV", "PCV"];
/** Red Alert 2 local variable 10: set when the player gets rich. */
const FLAG = Var.local(10);
const RICH: i64 = 5000;
const HEALTH: StaticArray<i32> = [Prop.HealthPercent];
const units = new StaticArray<i64>(64);

/** The human player, the money trigger and the frame it was paused. */
let human: House = 0;
let richTrigger: TriggerRef = 0;
let pausedAt: i64 = -1;

// ---------------------------------------------------------------- Handlers

function onBirth(e: BirthEvent): void {
  const built = Built.get(human) + 1;
  if (lastStatus() != Status.Ok) return;
  if (Built.set(human, built) == Status.Ok) {
    log("Trigger Lab: [Birth] tanks built so far (house variable): " + built.toString());
  }
}

function onFirstShot(e: FireEvent): void {
  const target = e.targetCell;
  log("Trigger Lab: [Fire, once] the first GI shot of the match, at cell " +
    Cell.x(target).toString() + "," + Cell.y(target).toString());
}

function onBoard(e: PassengerAddedEvent): void {
  log("Trigger Lab: [PassengerAdded] a soldier boarded; space left: " + e.space.toString());
}

function onRich(e: PollEvent): void {
  log("Trigger Lab: [PropertyPoll] money reached " + e.reading.toString() + " - setting local 10");
  FLAG.set(1);
}

function onFlag(e: PollEvent): void {
  log("Trigger Lab: [VariablePoll] local 10 is set; pausing the money trigger for a minute");
  FLAG.set(0);
  if (TriggerRef.setEnabled(richTrigger, false) == Status.Ok) pausedAt = <i64>frame();
}

function onMcvMoved(e: PollEvent): void {
  log("Trigger Lab: [DistancePoll, once] your MCV is 6 cells from where it started");
}

namedAction("OnBirth", onBirth);
namedAction("OnFirstShot", onFirstShot);
namedAction("OnBoard", onBoard);
namedAction("OnRich", onRich);
namedAction("OnFlag", onFlag);
namedAction("OnMcvMoved", onMcvMoved);

// ------------------------------------------------------------------ Start

/** Makes `trigger` and binds it to each of `names` the rules know. */
function bindAll(trigger: Trigger, names: StaticArray<string>): bool {
  const made = trigger.done();
  if (made == NO_HANDLE) return false;
  for (let i = 0; i < names.length; i++) {
    const ty = Rules.technoType(names[i]);
    if (ty != NO_HANDLE) TriggerRef.bind(made, ty);
  }
  return true;
}

/**
 * Whether `done` made `trigger` and bound it: the Rust SDK's `done` answers
 * an error for a failed binding too.
 */
function made(trigger: TriggerRef): bool {
  return trigger != NO_HANDLE && lastStatus() == Status.Ok;
}

function report(what: string, ok: bool): void {
  if (!ok) log("Trigger Lab: the " + what + " trigger was refused");
}

function startingMcv(house: House): Techno {
  for (let i = 0; i < MCVS.length; i++) {
    const ty = Rules.technoType(MCVS[i]);
    if (ty == NO_HANDLE) continue;
    const mcv = Find.objects().ownedBy(house).ofType(ty).first();
    if (mcv != NO_HANDLE) return mcv;
  }
  return NO_HANDLE;
}

onScenarioStart((): void => {
  const found = Find.houses().where(Prop.IsHuman, Op.Eq, 1).first();
  if (found == NO_HANDLE) {
    log("Trigger Lab: no human player");
    return;
  }
  human = found;
  log("Trigger Lab: build tanks, fire a GI, fill a transport, save 5000, move your MCV...");

  // Birth: an immediate action and a module action on one trigger.
  const birth = Trigger.on(Event.Birth)
    .where(Prop.Owner, Op.Eq, human)
    .immediate(ActionDef.log("Trigger Lab: [Birth] a tank rolled out (immediate action)"))
    .enqueue(onBirth);
  report("Birth", bindAll(birth, TANKS));

  // Fire bound to a weapon, once.
  const m60 = Rules.weapon("M60");
  if (m60 != NO_HANDLE) {
    report("Fire", made(Trigger.on(Event.Fire).bind(m60).once().enqueue(onFirstShot).done()));
  } else {
    log("Trigger Lab: weapon M60 not found");
  }

  // PassengerAdded on transports.
  report("PassengerAdded", bindAll(Trigger.on(Event.PassengerAdded).enqueue(onBoard), TRANSPORTS));

  // PropertyPoll on the house's money, checked every 2 seconds.
  const rich = Trigger.onProperty(Prop.Money, Op.Ge, RICH, 30).house(human).enqueue(onRich).done();
  if (rich != NO_HANDLE) richTrigger = rich;
  else log("Trigger Lab: the PropertyPoll trigger was refused");

  // HousePowerPoll with only an immediate action.
  report("HousePowerPoll", made(Trigger.onHousePower(Op.Lt, 0, 30)
    .house(human)
    .immediate(ActionDef.log("Trigger Lab: [HousePowerPoll] low power! Build a power plant."))
    .done()));

  // VariablePoll on local 10.
  report("VariablePoll", made(Trigger.onVariable(FLAG, Op.Eq, 1, 15).house(human).enqueue(onFlag).done()));

  // DistancePoll bound to one object: the starting MCV.
  const mcv = startingMcv(human);
  if (mcv != NO_HANDLE) {
    const cell: Cell = get(mcv, Prop.Cell);
    if (lastStatus() == Status.Ok) {
      report("DistancePoll", made(Trigger.onDistance(cell, Op.Gt, 6 * 256, 15)
        .bind(mcv)
        .once()
        .enqueue(onMcvMoved)
        .done()));
    }
  }
});

// ----------------------------------------------------------- Every second

/** The human player's units and their average health, read in one call. */
function armyReport(): void {
  const page = Find.objects()
    .ownedBy(human)
    .where(Prop.Category, Op.Ne, <i64>Category.Building)
    .page(units, 0);
  if (lastStatus() != Status.Ok) return;
  const count = page.count;
  if (count == 0) return;
  const list = new StaticArray<i64>(count);
  for (let i = 0; i < count; i++) list[i] = units[i];
  const health = new StaticArray<i64>(count);
  getMany(list, HEALTH, health);
  if (lastStatus() != Status.Ok) return;
  let total: i64 = 0;
  for (let i = 0; i < count; i++) total += health[i];
  log("Trigger Lab: [get_many] army report - " + count.toString() +
    (count == units.length ? "+ units" : " units") +
    ", average health " + (total / <i64>count).toString() + "%");
}

onFrame(new Every(15), (): void => {
  const now = <i64>frame();
  if (pausedAt >= 0 && now - pausedAt >= 900) {
    if (TriggerRef.setEnabled(richTrigger, true) == Status.Ok) {
      log("Trigger Lab: the money trigger is enabled again");
    }
    pausedAt = -1;
  }
  if (now > 0 && now % 900 == 0) armyReport();
});
