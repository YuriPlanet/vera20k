// "IniCounter", a demo mod for the INI names a module registers (RFC 0001
// section 6h), the same as the Rust SDK's `sdk/examples/ini-counter`: the
// modder writes INI (`rulesmd.ini` beside this folder), the module
// implements it.
//
// "While the house owns a radar, every N conscripts it trains earn it a
// free Rhino tank from its war factory." Each entry of the registry
// `[Demo.Counters]` names a counter section:
//
//   [Demo.Counters]
//   1=SovietDraft
//
//   [SovietDraft]
//   Count=E2              ; TechnoTypes counted when born for a house
//   Threshold=10          ; births per reward
//   Requires=NARADR       ; optional: counted only while the house owns one
//   Reward=HTNK           ; the free unit
//   Factory=NAWEAP        ; optional: the reward appears at it (else beside any building)
//   Houses=Russians       ; optional: countries the counter is for (default: all)
//   Stage=4               ; optional, demo only: see below
//
// - `IniEntry` (one per counter) parses the section and creates a `Birth`
//   trigger bound to the counted types; it queues `onCounted` with the
//   counter's index as parameter text.
// - The tallies are per house, in module memory (saved with the match).
// - A reward is created with `CreateObject`: first at the factory itself
//   (the engine answers `Unsupported` for a factory target so far), then
//   on a free cell around it.
// - Each reward raises two module events: the global `Demo.RewardGranted`
//   (value = counter index, house = the house), heard by a trigger made in
//   code with `Trigger.onModuleEvent`, and the object event
//   `Demo.RewardArrived` on the new unit, which the INI trigger
//   `[DemoRewardArrived]` hears through `Wasm.Tags` and answers with
//   `Wasm:onRewardArrived`.
// - `Stage=N` makes the demo play itself: once a computer house of the
//   counter's `Houses` has a building (from frame 150), the module gives it
//   the `Requires` and `Factory` buildings near it and N units of the first
//   `Count` type, whose `Birth` counts like a trained unit.
//
// The first time each step works it logs `DEMO-OK entry`, `DEMO-OK count`,
// `DEMO-OK reward` and `DEMO-OK event`.
import {
  Action, BirthEvent, Category, Cell, EventScope, Event, Every, Find, Global, House, Ini,
  IniEntryRecord, ModuleEvent, ModuleEventRecord, NO_HANDLE, NO_ID, NameKind, Op, Prop, Rules,
  Status, Target, Techno, Trigger, TypeRef,
  fail, frame, get, lastStatus, log, lookup, namedAction, onFrame, onScenarioStart,
} from "@vera20k/sdk";

const MAX_COUNTERS = 8;
/** Types in `Count=` and countries in `Houses=`; also a trigger's bindings. */
const MAX_TYPES = 8;
const MAX_HOUSES = 32;
const MAX_STAGE: i64 = 20;

/** Staging waits for a computer house's first building from this frame, takes any of its objects from the second and gives up after the third. */
const STAGE_FROM = 150;
const STAGE_ANY_OBJECT_FROM = 600;
const STAGE_UNTIL = 2400;

// ------------------------------------------------------------------ State

class Counter {
  count: TypeRef[] = [];
  /** Country type references (`Houses=`); none means every house. */
  countries: TypeRef[] = [];
  threshold: i64 = 0;
  /** `NO_HANDLE` when not configured. */
  requires: TypeRef = NO_HANDLE;
  reward: TypeRef = NO_HANDLE;
  factory: TypeRef = NO_HANDLE;
  stage: i64 = 0;
  /** Births counted so far, by house index. */
  tally: StaticArray<i64> = new StaticArray<i64>(MAX_HOUSES);
  /** Whether the demo staging is done for that house index. */
  staged: StaticArray<bool> = new StaticArray<bool>(MAX_HOUSES);
}

const counters: Counter[] = [];
/** `DEMO-OK` lines already written (bits below). */
let marks: i32 = 0;
let stagingDone = false;
/** Rewards logged so far; the log keeps only its latest lines. */
let rewardLines: i32 = 0;
/** The module events, set in initialization (`NO_ID` when refused). */
let rewardGranted: ModuleEvent = NO_ID;
let rewardArrived: ModuleEvent = NO_ID;

const MARK_ENTRY = 1;
const MARK_COUNT = 2;
const MARK_REWARD = 4;
const MARK_EVENT = 8;
const MARK_ARRIVED = 16;

/** Logs `line` the first time `bit` is marked. */
function mark(bit: i32, line: string): void {
  if ((marks & bit) == 0) {
    marks |= bit;
    log(line);
  }
}

// ------------------------------------------------------------ Registration
// Top-level code is the module's initialization, where INI names register.

if (Ini.registry("Demo.Counters", loadCounter) != Status.Ok) {
  log("IniCounter: another module already registered Demo.Counters");
}
rewardGranted = Ini.event("Demo.RewardGranted", EventScope.Global);
if (rewardGranted == NO_ID) log("IniCounter: another module already registered Demo.RewardGranted");
rewardArrived = Ini.event("Demo.RewardArrived", EventScope.Object);
if (rewardArrived == NO_ID) log("IniCounter: another module already registered Demo.RewardArrived");

// The INI trigger `[DemoRewardArrived]` calls it (`Action1=Wasm:onRewardArrived`).
namedAction("onRewardArrived", onRewardArrived);

/** Stops the module with a message naming the section and key, as a handler should for a bad value (RFC 0001 section 6h). */
function stop(section: string, key: string, why: string): void {
  fail("IniCounter: [" + section + "] " + key + ": " + why);
}

/** `[section] key`'s value without a comment or surrounding spaces; `""` when missing or empty. */
function value(section: string, key: string): string {
  const text = Rules.get(section, key);
  const status = lastStatus();
  if (status == Status.NotFound) return "";
  if (status != Status.Ok) stop(section, key, "the value cannot be read");
  return Ini.stripComment(text);
}

function techno(section: string, key: string, name: string): TypeRef {
  const t = Rules.technoType(name);
  if (t == NO_HANDLE) stop(section, key, "not a TechnoType");
  return t;
}

/** An optional TechnoType key; `NO_HANDLE` when missing. */
function optionalTechno(section: string, key: string): TypeRef {
  const name = value(section, key);
  return name.length ? techno(section, key, name) : NO_HANDLE;
}

/** One `Demo.Counters` entry: parse its section, make its `Birth` trigger. */
function loadCounter(record: IniEntryRecord): void {
  const section = record.entryName;
  const index = counters.length;
  if (index == MAX_COUNTERS) stop("Demo.Counters", section, "more than 8 counters");
  const counter = new Counter();

  const list = value(section, "Count");
  if (!list.length) stop(section, "Count", "missing");
  const names = Ini.split(list);
  for (let i = 0; i < names.length; i++) {
    if (!names[i].length) continue;
    if (counter.count.length == MAX_TYPES) stop(section, "Count", "more than 8 types");
    counter.count.push(techno(section, "Count", names[i]));
  }
  if (counter.count.length == 0) stop(section, "Count", "no type");

  counter.threshold = Ini.int(value(section, "Threshold"), 0);
  if (counter.threshold < 1) stop(section, "Threshold", "needs a number of 1 or more");

  const reward = value(section, "Reward");
  if (!reward.length) stop(section, "Reward", "missing");
  counter.reward = techno(section, "Reward", reward);
  counter.requires = optionalTechno(section, "Requires");
  counter.factory = optionalTechno(section, "Factory");

  const houses = value(section, "Houses");
  if (houses.length) {
    const countries = Ini.split(houses);
    for (let i = 0; i < countries.length; i++) {
      if (!countries[i].length) continue;
      if (counter.countries.length == MAX_TYPES) stop(section, "Houses", "more than 8 countries");
      const id = lookup(NameKind.Country, countries[i]);
      if (id == NO_ID) stop(section, "Houses", "not a country");
      counter.countries.push(TypeRef.make(NameKind.Country, id));
    }
  }

  const stage = value(section, "Stage");
  if (stage.length) {
    counter.stage = Ini.int(stage, -1);
    if (counter.stage < 0 || counter.stage > MAX_STAGE) stop(section, "Stage", "needs a number from 0 to 20");
  }

  // One trigger for the counter: `Birth` of any counted type, queueing
  // `onCounted` with the counter's index as its parameter text.
  const trigger = Trigger.on(Event.Birth);
  for (let i = 0; i < counter.count.length; i++) trigger.bind(counter.count[i]);
  if (trigger.enqueueWith(onCounted, Target.Subject, index.toString()).done() == NO_HANDLE) {
    stop(section, "Count", "the Birth trigger could not be made");
  }

  counters.push(counter);
  log("IniCounter: counter " + index.toString() + " [" + section + "]: every " +
    counter.threshold.toString() + " births earn a reward");
  mark(MARK_ENTRY, "DEMO-OK entry");
}

/** After the `IniEntry` records: the code trigger that hears the global event. */
onScenarioStart((): void => {
  if (rewardGranted == NO_ID) return;
  if (Trigger.onModuleEvent(rewardGranted).enqueue(onRewardGranted).done() == NO_HANDLE) {
    log("IniCounter: the Demo.RewardGranted listener could not be made");
  }
});

// ----------------------------------------------------------------- Houses

/** `house`'s index, the slot of its tallies; −1 for none. */
function houseSlot(house: House): i32 {
  const index = get(house, Prop.Index);
  if (lastStatus() != Status.Ok || index < 0 || index >= MAX_HOUSES) return -1;
  return <i32>index;
}

/** Whether `counter` is for `house`'s country. */
function applies(counter: Counter, house: House): bool {
  if (counter.countries.length == 0) return true;
  const country = get(house, Prop.Country);
  if (lastStatus() != Status.Ok) return false;
  return counter.countries.includes(country);
}

/** `house`'s first object of type `kind`, or `NO_HANDLE`. */
function owned(house: House, kind: TypeRef): Techno {
  return Find.objects().ownedBy(house).ofType(kind).first();
}

/** Whether `house` meets `counter`'s `Requires=`. */
function requirementMet(counter: Counter, house: House): bool {
  return counter.requires == NO_HANDLE || owned(house, counter.requires) != NO_HANDLE;
}

function firstBuilding(house: House): Techno {
  return Find.objects().ownedBy(house).where(Prop.Category, Op.Eq, <i64>Category.Building).first();
}

/** `object`'s cell, or `NO_HANDLE`. */
function cellOf(object: Techno): Cell {
  if (object == NO_HANDLE) return NO_HANDLE;
  const cell = get(object, Prop.Cell);
  return lastStatus() == Status.Ok ? cell : NO_HANDLE;
}

/** Creates a `kind` for `house` on the first free cell of the rings `from..to` cells around `center`; `NO_HANDLE` for none. */
function placeNear(kind: TypeRef, house: House, center: Cell, from: i32, to: i32): Techno {
  for (let ring = from; ring <= to; ring++) {
    for (let dy = -ring; dy <= ring; dy++) {
      for (let dx = -ring; dx <= ring; dx++) {
        if (abs(dx) != ring && abs(dy) != ring) continue; // Inside the ring.
        const x = Cell.x(center) + dx;
        const y = Cell.y(center) + dy;
        if (x < 1 || x > 511 || y < 1 || y > 511) continue;
        // An occupied or off-map cell turns the object down; try on.
        const object = Action.createObject(Cell.at(x, y), kind, house);
        if (object != NO_HANDLE) return object;
      }
    }
  }
  return NO_HANDLE;
}

// --------------------------------------------------------------- Counting

/** A counted type was born: count it for its house while the requirement holds, and grant the reward at the threshold. */
function onCounted(event: BirthEvent): void {
  const index = <i32>Ini.int(event.record.argsText, -1);
  if (index < 0 || index >= counters.length) return;
  // The object may be gone by now (killed in the frame it appeared).
  let house = Techno.owner(event.object);
  if (house == NO_HANDLE) house = event.record.house;
  if (house == NO_HANDLE) return;
  const slot = houseSlot(house);
  if (slot < 0) return;
  const counter = counters[index];
  if (!applies(counter, house) || !requirementMet(counter, house)) return;
  counter.tally[slot] += 1;
  mark(MARK_COUNT, "DEMO-OK count");
  // A reward with no room stays due: the next birth tries again.
  if (counter.tally[slot] >= counter.threshold && grant(index, house)) {
    counter.tally[slot] -= counter.threshold;
  }
}

/** Creates counter `index`'s reward for `house` and raises the events; `false` when there was no room for it. */
function grant(index: i32, house: House): bool {
  const counter = counters[index];
  const factory = counter.factory == NO_HANDLE ? NO_HANDLE : owned(house, counter.factory);
  // Out through the factory's door; this engine does not take a factory as
  // CreateObject's target yet (`Unsupported`), so a cell beside it.
  let reward = factory == NO_HANDLE ? NO_HANDLE : Action.createObject(factory, counter.reward, house);
  if (reward == NO_HANDLE) {
    const anchor = factory == NO_HANDLE ? firstBuilding(house) : factory;
    const center = cellOf(anchor);
    if (center != NO_HANDLE) reward = placeNear(counter.reward, house, center, 2, 8);
  }
  if (reward == NO_HANDLE) return false;
  mark(MARK_REWARD, "DEMO-OK reward");
  if (rewardLines < 8) {
    rewardLines++;
    log("IniCounter: counter " + index.toString() + " granted a reward to house " + get(house, Prop.Index).toString());
  }
  if (rewardArrived != NO_ID) Action.raiseEvent(rewardArrived, reward, NO_HANDLE, index, 0);
  if (rewardGranted != NO_ID) Action.raiseEvent(rewardGranted, Global.REF, NO_HANDLE, index, 0, house);
  return true;
}

/** The code trigger on the global `Demo.RewardGranted`. */
function onRewardGranted(event: ModuleEventRecord): void {
  if ((marks & MARK_EVENT) == 0) {
    log("IniCounter: Demo.RewardGranted heard (counter " + event.value.toString() + ", house " +
      get(event.house, Prop.Index).toString() + ")");
  }
  mark(MARK_EVENT, "DEMO-OK event");
}

/** The INI trigger on the object event `Demo.RewardArrived` (`rulesmd.ini`, `[DemoRewardArrived]`). */
function onRewardArrived(event: ModuleEventRecord): void {
  mark(MARK_ARRIVED, "IniCounter: an INI trigger heard Demo.RewardArrived on the new unit");
}

// ---------------------------------------------------------------- Staging

const HOUSES = new StaticArray<i64>(MAX_HOUSES);

/** Demo only (`Stage=`): gives each computer house of a counter's `Houses` what the counter needs, then `Stage` counted units. */
function tick(): void {
  const now = frame();
  if (stagingDone || now < STAGE_FROM || now > STAGE_UNTIL) return;
  const page = Find.houses()
    .where(Prop.IsHuman, Op.Eq, 0)
    .where(Prop.IsDefeated, Op.Eq, 0)
    .page(HOUSES, 0);
  if (lastStatus() != Status.Ok) return;
  const found = page.count;
  let pending = false;
  for (let index = 0; index < counters.length; index++) {
    const counter = counters[index];
    // Without `Houses=` the computer houses include the neutral ones.
    if (counter.stage == 0 || counter.countries.length == 0) continue;
    for (let i = 0; i < found; i++) {
      const house: House = HOUSES[i];
      if (!applies(counter, house)) continue;
      const slot = houseSlot(house);
      if (slot < 0 || counter.staged[slot]) continue;
      if (stage(counter, house, now)) counter.staged[slot] = true;
      else pending = true;
    }
  }
  if (!pending) stagingDone = true;
}

/** Stages `counter` for `house`; `false` while the house has nothing to stage beside. */
function stage(counter: Counter, house: House, now: i32): bool {
  let anchor = firstBuilding(house);
  if (anchor == NO_HANDLE && now >= STAGE_ANY_OBJECT_FROM) anchor = Find.objects().ownedBy(house).first();
  const center = cellOf(anchor);
  if (center == NO_HANDLE) return false;
  const buildings: TypeRef[] = [counter.requires, counter.factory];
  for (let i = 0; i < buildings.length; i++) {
    const building = buildings[i];
    if (building != NO_HANDLE && owned(house, building) == NO_HANDLE &&
      placeNear(building, house, center, 4, 14) == NO_HANDLE) {
      log("IniCounter: no room to stage a building");
    }
  }
  let made = 0;
  for (let i: i64 = 0; i < counter.stage; i++) {
    if (placeNear(counter.count[0], house, center, 3, 10) != NO_HANDLE) made++;
  }
  log("IniCounter: staged " + made.toString() + " units for house " + get(house, Prop.Index).toString());
  return true;
}

onFrame(new Every(15), tick);
