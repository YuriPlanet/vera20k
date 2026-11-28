// "Veteran's Fury", a playtest `rules.wasm` that combines several module
// features in one skirmish, the same as the Rust SDK's
// `sdk/examples/veteran-fury`:
//
// - Bounty: a human player's unit that kills a common infantry or tank
//   earns its house 150 credits (a `Killed` trigger bound to types).
// - Veterans: the third kill of a unit promotes it: firepower x1.5 and
//   armor x1.25 for good, fully repaired (an object variable counts kills,
//   modifiers apply the boost).
// - Last stand: a human player's tank that drops below 30% health gets
//   armor x3 for 6 s and 50% more speed for 10 s, once per tank (a
//   `TakeDamage` trigger with property and house clauses).
// - Waves: every minute each computer opponent gets a few tanks beside
//   its base and sends them at the first human player's base with an
//   attack-move order; each wave is larger (a global variable counts).
// - Interest: every minute each human player earns 5% of their money, at
//   most 500 credits.
import {
  Action, Category, Cell, Event, Every, Find, House, KilledEvent, Modifier, NO_HANDLE, Op, Order,
  Prop, Rules, Status, TakeDamageEvent, Techno, Trigger, TriggerRef, TypeRef, fixed, frame, get,
  lastStatus, log, namedAction, onFrame, onScenarioStart,
} from "@vera20k/sdk";
import { Kills, Rallied, Wave } from "./generated/vars";

const BOUNTY: i32 = 150;
const PROMOTE_AT: i64 = 3;
/** Kills that pay a bounty: the common infantry and tanks of both sides. */
const PREY: StaticArray<string> = [
  "E1", "E2", "GGI", "FLAKT", "SHK", "INIT", "MTNK", "HTNK", "FV", "HTK", "APOC", "LTNK",
];
/** Tanks that make a last stand. */
const TANKS: StaticArray<string> = ["MTNK", "HTNK", "FV", "APOC", "TNKD", "LTNK"];
/** Frames between waves (15 frames a second). */
const WAVE_EVERY: i32 = 900;
/** Construction yards of the three sides. */
const YARDS: StaticArray<string> = ["GACNST", "NACNST", "YACNST"];
/** No cell (`Option<Cell>::None` in the Rust version). */
const NO_CELL: Cell = 0;

const houses = new StaticArray<i64>(16);
const tanks = new StaticArray<Techno>(6);

// ----------------------------------------------------- Bounty and veterans

function onKill(e: KilledEvent): void {
  const killer = e.killer;
  const house = Techno.owner(killer);
  if (lastStatus() != Status.Ok) return; // No killer, or it died in the same frame.
  const isHuman = get(house, Prop.IsHuman);
  if (lastStatus() != Status.Ok || isHuman != 1) return;
  Action.addMoney(house, BOUNTY);
  const kills = Kills.get(killer) + 1;
  if (lastStatus() != Status.Ok) return;
  if (Kills.set(killer, kills) != Status.Ok) return;
  if (kills == PROMOTE_AT) {
    Modifier.multiply(killer, Prop.FirepowerMultiplier, fixed(1.5), -1);
    Modifier.multiply(killer, Prop.ArmorMultiplier, fixed(1.25), -1);
    const strength = get(killer, Prop.Strength);
    if (lastStatus() == Status.Ok) Action.heal(killer, <i32>strength);
    log("Veteran's Fury: a unit made its 3rd kill and became a veteran!");
  } else {
    log("Veteran's Fury: +150 bounty (that unit's kills: " + kills.toString() + ")");
  }
}

// -------------------------------------------------------------- Last stand

function onLastStand(e: TakeDamageEvent): void {
  const tank = e.victim;
  // Gone when the hit killed it; 1 once it has made its stand.
  const rallied = Rallied.get(tank);
  if (lastStatus() != Status.Ok || rallied != 0 || Rallied.set(tank, 1) != Status.Ok) return;
  // Armor x3 rather than `Unkillable`, which needs a Phobos build.
  Modifier.multiply(tank, Prop.ArmorMultiplier, fixed(3.0), 90);
  Modifier.multiply(tank, Prop.SpeedMultiplier, fixed(1.5), 150);
  log("Veteran's Fury: last stand! A tank is armor x3 for 6 s, 50% faster for 10 s.");
}

namedAction("OnKill", onKill);
namedAction("OnLastStand", onLastStand);

// ------------------------------------------------------------------ Start

/** Binds `trigger` to each of `names` the rules know. */
function bindTypes(trigger: TriggerRef, names: StaticArray<string>): void {
  for (let i = 0; i < names.length; i++) {
    const ty = Rules.technoType(names[i]);
    if (ty != NO_HANDLE) TriggerRef.bind(trigger, ty);
  }
}

onScenarioStart((): void => {
  log("Veteran's Fury: +150 per kill; 3 kills make a veteran (x1.5 firepower).");
  log("Tanks below 30% make a last stand; enemy waves every minute; 5% interest a minute.");
  const bounty = Trigger.on(Event.Killed).enqueue(onKill).done();
  if (bounty != NO_HANDLE) bindTypes(bounty, PREY);
  else log("Veteran's Fury: the bounty trigger was refused");
  const stand = Trigger.on(Event.TakeDamage)
    .where(Prop.HealthPercent, Op.Lt, 30)
    .whereHouse(Prop.IsHuman, Op.Eq, 1)
    .enqueue(onLastStand)
    .done();
  if (stand != NO_HANDLE) bindTypes(stand, TANKS);
  else log("Veteran's Fury: the last-stand trigger was refused");
});

// ------------------------------------------------------------------- Waves

function cellOf(object: i64): Cell {
  if (object == NO_HANDLE) return NO_CELL;
  const cell: Cell = get(object, Prop.Cell);
  return lastStatus() == Status.Ok ? cell : NO_CELL;
}

/** The cell of `house`'s first construction yard. */
function yardCell(house: House): Cell {
  for (let i = 0; i < YARDS.length; i++) {
    const ty = Rules.technoType(YARDS[i]);
    if (ty == NO_HANDLE) continue;
    const cell = cellOf(Find.objects().ownedBy(house).ofType(ty).first());
    if (cell != NO_CELL) return cell;
  }
  return NO_CELL;
}

/** The cell of the first human player's construction yard, or else of their first building. */
function humanBase(): Cell {
  const human = Find.houses().where(Prop.IsHuman, Op.Eq, 1).first();
  if (human == NO_HANDLE) return NO_CELL;
  const yard = yardCell(human);
  if (yard != NO_CELL) return yard;
  return cellOf(Find.objects()
    .ownedBy(human)
    .where(Prop.Category, Op.Eq, <i64>Category.Building)
    .first());
}

function statusName(status: Status): string {
  switch (status) {
    case Status.Ok: return "Ok";
    case Status.Gone: return "Gone";
    case Status.NotFound: return "NotFound";
    case Status.Refused: return "Refused";
    default: return "Unsupported";
  }
}

function clamp(value: i32, lo: i32, hi: i32): i32 {
  return value < lo ? lo : value > hi ? hi : value;
}

/**
 * Creates up to `size` tanks of `kind` around `base` for `house` and
 * orders them to attack-move to `target`; returns how many it made.
 */
function sendWave(house: House, base: Cell, kind: TypeRef, size: i32, target: Cell): i32 {
  let refused = Status.Ok;
  let tried = false;
  let made = 0;
  // A ring of cells around the base; occupied ones refuse the object.
  for (let dy = -4; dy <= 4; dy++) {
    for (let dx = -4; dx <= 4; dx++) {
      if (made == size) break;
      if (abs<i32>(dx) < 3 && abs<i32>(dy) < 3) continue; // Inside the building's footprint, probably.
      const x = clamp(Cell.x(base) + dx, 1, 500);
      const y = clamp(Cell.y(base) + dy, 1, 500);
      const tank = Action.createObject(Cell.at(x, y), kind, house);
      if (tank != NO_HANDLE) {
        tanks[made++] = tank;
      } else {
        refused = lastStatus();
        tried = true;
      }
    }
  }
  if (made == 0) {
    log("Veteran's Fury: no tank could be placed near (" + Cell.x(base).toString() + "," +
      Cell.y(base).toString() + "): " + (tried ? statusName(refused) : "no cell tried"));
  } else {
    const sent = new StaticArray<Techno>(made);
    for (let i = 0; i < made; i++) sent[i] = tanks[i];
    const status = House.order(house, Order.attackMove(sent, target));
    if (status != Status.Ok) log("Veteran's Fury: attack order refused: " + statusName(status));
  }
  return made;
}

function waves(): void {
  if (frame() < WAVE_EVERY) return;
  let wave = Wave.get();
  if (lastStatus() != Status.Ok) wave = 0;
  wave += 1;
  Wave.set(wave);
  const target = humanBase();
  if (target == NO_CELL) {
    log("Veteran's Fury: wave skipped - no human building found");
    return;
  }
  const size = <i32>min<i64>(1 + wave, 6);
  const kind = Rules.technoType(wave % 2 == 1 ? "MTNK" : "HTNK");
  if (kind == NO_HANDLE) {
    log("Veteran's Fury: wave skipped - tank type not found");
    return;
  }
  const page = Find.houses()
    .where(Prop.IsHuman, Op.Eq, 0)
    .where(Prop.IsDefeated, Op.Eq, 0)
    .page(houses, 0);
  if (lastStatus() != Status.Ok) {
    log("Veteran's Fury: wave skipped - house search failed");
    return;
  }
  const count = page.count;
  // Computer opponents are the computer houses with a construction yard;
  // that leaves out the neutral and civilian houses (the engine does not
  // answer `IsCivilian` yet).
  let opponents = 0;
  let sent = 0;
  for (let i = 0; i < count; i++) {
    const house: House = houses[i];
    const base = yardCell(house);
    if (base != NO_CELL) {
      opponents++;
      sent += sendWave(house, base, kind, size, target);
    }
  }
  if (opponents == 0) {
    log("Veteran's Fury: wave skipped - no computer opponent has a construction yard yet");
  }
  if (sent > 0) {
    log("Veteran's Fury: wave " + wave.toString() + " - " + sent.toString() +
      " enemy tanks are heading for your base!");
  }
}

// ---------------------------------------------------------------- Interest

function interest(): void {
  if (frame() == 0) return;
  const page = Find.houses().where(Prop.IsHuman, Op.Eq, 1).page(houses, 0);
  if (lastStatus() != Status.Ok) return;
  const count = page.count;
  for (let i = 0; i < count; i++) {
    const house: House = houses[i];
    const money = get(house, Prop.Money);
    if (lastStatus() != Status.Ok) continue;
    const paid = min<i64>(money / 20, 500);
    if (paid > 0 && Action.addMoney(house, <i32>paid) == Status.Ok) {
      log("Veteran's Fury: interest +" + paid.toString());
    }
  }
}

/** One `onFrame` callback: every 900 frames it pays interest and sends a wave. */
onFrame(new Every(900), (): void => {
  interest();
  if (frame() % WAVE_EVERY == 0) waves();
});
