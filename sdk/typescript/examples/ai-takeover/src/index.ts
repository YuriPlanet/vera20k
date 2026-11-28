// "AI Takeover", a demo `rules.wasm` for driving a house (RFC 0001
// section 6i), the same as the Rust SDK's `sdk/examples/ai-takeover`: the
// module claims every computer house and plays it with a small scripted
// AI, through orders only (no `CreateObject`).
//
// - Claims: it writes `ControlDomains` = 15 (all four `Domain` bits) on
//   each computer house that owns a Construction Yard or an MCV, reads it
//   back, and checks that the same write on the human house is turned
//   down (`Status.Refused`). With the mod's `[VeraWasm] Controller=yes`
//   and a "mod AI" seat the house starts claimed already; the write then
//   only confirms it.
// - The house: deploy the MCV (`DeployMcv`), then a build order of power
//   plant, barracks, refinery and war factory (Soviet or Allied, from the
//   MCV or yard the house owns), each `QueueProduction` once the building
//   factory is idle (`Factory.Building.*`) and placed with
//   `PlaceReadyBuilding` at a cell `find` answers `Placeable=1` for, near
//   the yard. With a barracks and a war factory it trains infantry and
//   tanks and lets each ready one out (`TakeProduct`); once the war factory
//   stands and the army has three or more, it sends it at the human
//   player's base (`AttackMove`), again every 450 frames. `find` also
//   returns the product a factory holds, off the map, so a building counts
//   as standing only beyond the one the building factory holds.
// - Results: a `Commanded` trigger bound to the unit types and a global
//   `OrderRefused` trigger deliver records to their handlers; a `Birth`
//   trigger on the trained types tells it a unit left the factory.
//
// Work runs in `onFrame` every 15 frames, one claimed house per visit.
// It logs `DEMO-OK <feature>` the first time each feature works (the
// engine test `sim::ext_modules::tests::ini_demo_mods_reach_their_marks`
// checks them) and a status line every 600 frames. The log never says
// the word the test treats as a failure, so a turned-down order is
// "order-rejected" here.
import {
  Category, Cell, Command, Event, EventRecord, Every, Find, House, NO_HANDLE, ORDER_UNITS, Op,
  Order, Prop, Ref, Rules, Status, Techno, Trigger, TypeRef,
  frame, get, lastStatus, log, onFrame, onScenarioStart, set,
} from "@vera20k/sdk";

/** Every `Domain` bit: Production, Forces, Abilities, SuperWeapons. */
const ALL_DOMAINS: i64 = 0b1111;
/** Houses this module drives at most. */
const MAX_SEATS: i32 = 4;
/** Units an attack waits for. */
const ATTACK_SIZE: i32 = 3;
/** Frames between attack orders. */
const ATTACK_EVERY: i64 = 450;
/** Units the module trains per house at most, so the credits last. */
const MAX_TRAINED: i64 = 24;
/** `RefuseReason` (appendix 8.3, enum 26) values it reads. */
const REASON_CANNOT_PLACE: i64 = 7;
/** `Factory.<Kind>` indices `TakeProduct` takes. */
const KIND_INFANTRY: i64 = 2;
const KIND_VEHICLE: i64 = 3;

// ------------------------------------------------------------------- Sides

const SIDE_NAMES: StaticArray<string> = ["Soviet", "Allied"];
const SIDE_COUNT: i32 = 2;

const BUILD_STEPS: i32 = 4;
const STEP_BARRACKS: i32 = 1;
const STEP_WAR_FACTORY: i32 = 3;

/** Type slots per side: MCV, yard, the build order (power plant, barracks, refinery, war factory), infantry, vehicle. */
const SLOT_MCV: i32 = 0;
const SLOT_YARD: i32 = 1;
const SLOT_BUILD: i32 = 2;
const SLOT_INFANTRY: i32 = SLOT_BUILD + BUILD_STEPS;
const SLOT_VEHICLE: i32 = SLOT_INFANTRY + 1;
const SLOTS: i32 = SLOT_VEHICLE + 1;

/** TechnoType names by side and slot. */
const TYPE_NAMES: StaticArray<string> = [
  "SMCV", "NACNST", "NAPOWR", "NAHAND", "NAREFN", "NAWEAP", "E2", "HTNK",
  "AMCV", "GACNST", "GAPOWR", "GAPILE", "GAREFN", "GAWEAP", "E1", "MTNK",
];

/** Type references by side and slot, resolved once; 0 when the rules lack the type. */
const TYPES = new StaticArray<TypeRef>(SIDE_COUNT * SLOTS);

function ty(side: i32, slot: i32): TypeRef {
  return TYPES[side * SLOTS + slot];
}

function resolveTypes(): void {
  for (let i = 0; i < SIDE_COUNT * SLOTS; i++) {
    const found = Rules.technoType(TYPE_NAMES[i]);
    if (found != NO_HANDLE) TYPES[i] = found;
    else log("ai-takeover: the rules lack " + TYPE_NAMES[i]);
  }
}

// ------------------------------------------------------------------- Seats

/** Claimed houses and what the module remembers of each. */
let seats: i32 = 0;
const seatHouseOf = new StaticArray<House>(MAX_SEATS);
const seatSideOf = new StaticArray<i32>(MAX_SEATS);
/** Frame of the last `DeployMcv`, 0 before the first; attempts so far. */
const deployFrame = new StaticArray<i64>(MAX_SEATS);
const deployTries = new StaticArray<i64>(MAX_SEATS);
/** The building last ordered placed, and where; 0 when none waits. */
const placingType = new StaticArray<i64>(MAX_SEATS);
const placingCell = new StaticArray<i64>(MAX_SEATS);
/** Placements the house could not make (it then tries other cells). */
const placeFails = new StaticArray<i64>(MAX_SEATS);
/** Whether the infantry and vehicle factories read ready last visit. */
const infantryReady = new StaticArray<bool>(MAX_SEATS);
const vehicleReady = new StaticArray<bool>(MAX_SEATS);
/** Units queued so far, and whether the module queued any. */
const training = new StaticArray<i64>(MAX_SEATS);
/** Frame of the last attack order, 0 before the first. */
const attackFrame = new StaticArray<i64>(MAX_SEATS);
/** Whether the early tank order (to be turned down) went out. */
const provoked = new StaticArray<bool>(MAX_SEATS);
/** Whether the native AI check ran. */
const idleChecked = new StaticArray<bool>(MAX_SEATS);

/** Visits so far, which picks the seat of each visit. */
let visits: i64 = 0;

/** `find` buffers. */
const HOUSES = new StaticArray<i64>(16);
const PAIR = new StaticArray<i64>(2);
const UNITS_PAGE = new StaticArray<i64>(16);
const CELLS = new StaticArray<i64>(64);
const BUILDINGS = new StaticArray<i64>(32);
const ARMY = new StaticArray<Techno>(ORDER_UNITS);
const ONE_UNIT = new StaticArray<Techno>(1);

/** `prop` of `reference`, or `otherwise` when it cannot be read. */
function getOr(reference: Ref, prop: Prop, otherwise: i64): i64 {
  const value = get(reference, prop);
  return lastStatus() == Status.Ok ? value : otherwise;
}

function seatCount(): i32 {
  return min(seats, MAX_SEATS);
}

/** The seat of `house`, −1 when the module does not drive it. */
function seatOf(house: i64): i32 {
  const count = seatCount();
  for (let seat = 0; seat < count; seat++) if (seatHouseOf[seat] == house) return seat;
  return -1;
}

/** The seat that owns `unit`, or −1. */
function seatOfOwner(unit: Techno): i32 {
  const owner = get(unit, Prop.Owner);
  return lastStatus() == Status.Ok ? seatOf(owner) : -1;
}

// ------------------------------------------------------------------- Marks

let marks: i64 = 0;

const MARK_CLAIM: i64 = 1 << 0;
const MARK_DEPLOY: i64 = 1 << 1;
const MARK_BUILD: i64 = 1 << 2;
const MARK_PLACE: i64 = 1 << 3;
const MARK_TRAIN: i64 = 1 << 4;
const MARK_ATTACK: i64 = 1 << 5;
const MARK_REJECTED: i64 = 1 << 6;
const MARK_IDLE: i64 = 1 << 7;
const MARK_BASE: i64 = 1 << 8;
/** One-time notes that are not marks. */
const NOTE_NO_CELL: i64 = 1 << 16;
const NOTE_NATIVE_BUILT: i64 = 1 << 17;
const NOTE_DEPLOY_HEARD: i64 = 1 << 18;
const NOTE_NO_SEAT: i64 = 1 << 19;
const NOTE_PLAYER_DEFEATED: i64 = 1 << 20;

/** Logs `line` the first time `bit` is marked. */
function mark(bit: i64, line: string): void {
  if ((marks & bit) == 0) {
    marks |= bit;
    log(line);
  }
}

/** Detail lines about orders not carried out, kept few: the log holds the latest 256 lines. */
let detailLines: i64 = 0;
const MAX_DETAIL_LINES: i64 = 8;

// ---------------------------------------------------------------- Searches

/** The first object of `t` that `house` owns, or `NO_HANDLE`. */
function owned(house: House, t: TypeRef): Techno {
  if (t == NO_HANDLE) return NO_HANDLE;
  return Find.objects().ownedBy(house).ofType(t).first();
}

/**
 * Whether `house` has a `building` standing on the map. `find` also
 * returns the product a factory holds (it exists off the map from the
 * start of production), so the one the building factory holds is not
 * counted.
 */
function standing(house: House, building: TypeRef): bool {
  if (building == NO_HANDLE) return false;
  const page = Find.objects().ownedBy(house).ofType(building).page(PAIR, 0);
  if (lastStatus() != Status.Ok) return false;
  const count = page.count;
  const held = getOr(house, Prop.Factory_Building_Type, 0) == building;
  return count > (held ? 1 : 0);
}

/** The house's units of `t`, appended to `ARMY` from `len`; the new length. */
function collect(house: House, t: TypeRef, from: i32): i32 {
  let len = from;
  if (t == NO_HANDLE) return len;
  const find = Find.objects().ownedBy(house).ofType(t);
  let after: i64 = 0;
  while (len < ARMY.length) {
    const page = find.page(UNITS_PAGE, after);
    if (lastStatus() != Status.Ok) break;
    const count = page.count;
    const next = page.next;
    for (let i = 0; i < count; i++) {
      if (len < ARMY.length) ARMY[len++] = UNITS_PAGE[i];
    }
    if (next == 0) break;
    after = next;
  }
  return len;
}

/** The first `len` units of `ARMY`, for an order. */
function armyOf(len: i32): StaticArray<Techno> {
  const units = new StaticArray<Techno>(len);
  for (let i = 0; i < len; i++) units[i] = ARMY[i];
  return units;
}

/** The cell of the human player's base: a building of theirs, else any of their objects; `NO_HANDLE` for none. */
function enemyBase(): Cell {
  const human: House = Find.houses().where(Prop.IsHuman, Op.Eq, 1).first();
  if (human == NO_HANDLE) return NO_HANDLE;
  let target = Find.objects().ownedBy(human).where(Prop.Category, Op.Eq, <i64>Category.Building).first();
  if (target == NO_HANDLE) target = Find.objects().ownedBy(human).first();
  if (target == NO_HANDLE) return NO_HANDLE;
  const cell = get(target, Prop.Cell);
  return lastStatus() == Status.Ok ? cell : NO_HANDLE;
}

// ------------------------------------------------------------------ Claims

/** The side whose MCV or yard `house` owns, or −1. */
function sideOf(house: House): i32 {
  for (let side = 0; side < SIDE_COUNT; side++) {
    if (owned(house, ty(side, SLOT_YARD)) != NO_HANDLE || owned(house, ty(side, SLOT_MCV)) != NO_HANDLE) return side;
  }
  return -1;
}

/** Claims every computer house with a base, once some exist. */
function claimSeats(): void {
  const find = Find.houses().where(Prop.IsHuman, Op.Eq, 0);
  let after: i64 = 0;
  let allReadBack = true;
  while (true) {
    const page = find.page(HOUSES, after);
    if (lastStatus() != Status.Ok) break;
    const count = page.count;
    const next = page.next;
    for (let i = 0; i < count; i++) {
      const house: House = HOUSES[i];
      const taken = seatCount();
      if (taken == MAX_SEATS || seatOf(house) >= 0) continue;
      // Civilian and neutral houses own neither an MCV nor a yard.
      const side = sideOf(house);
      if (side < 0) continue;
      if (set(house, Prop.ControlDomains, ALL_DOMAINS) != Status.Ok) {
        allReadBack = false;
        continue;
      }
      const held = getOr(house, Prop.ControlDomains, 0);
      if (held != ALL_DOMAINS) {
        allReadBack = false;
        continue;
      }
      seatHouseOf[taken] = house;
      seatSideOf[taken] = side;
      seats = taken + 1;
      log("ai-takeover: playing house " + getOr(house, Prop.Index, -1).toString() + " (" + SIDE_NAMES[side] +
        "), domains " + held.toString());
    }
    if (next == 0) break;
    after = next;
  }
  if (seatCount() == 0) return;
  // A player's house takes no claim.
  const human: House = Find.houses().where(Prop.IsHuman, Op.Eq, 1).first();
  let humanDeclined = false;
  if (human != NO_HANDLE) {
    const status = set(human, Prop.ControlDomains, ALL_DOMAINS);
    if (status == Status.Refused) humanDeclined = true;
    else if (status == Status.Ok) {
      // Not expected: give it back at once.
      set(human, Prop.ControlDomains, 0);
      log("ai-takeover: the human house took the claim");
    }
  }
  if (allReadBack && humanDeclined) mark(MARK_CLAIM, "DEMO-OK claim");
}

// ---------------------------------------------------------------- Triggers

function makeTriggers(): void {
  // `Commanded` is object-bound: bind the unit types the module orders.
  const commanded = Trigger.on(Event.Commanded).enqueue(onCommanded);
  const birth = Trigger.on(Event.Birth).enqueue(onBirth);
  for (let side = 0; side < SIDE_COUNT; side++) {
    for (let s = 0; s < 3; s++) {
      const slot = s == 0 ? SLOT_MCV : s == 1 ? SLOT_INFANTRY : SLOT_VEHICLE;
      const unit = ty(side, slot);
      if (unit == NO_HANDLE) continue;
      commanded.bind(unit);
      if (slot != SLOT_MCV) birth.bind(unit);
    }
  }
  const madeCommanded = commanded.done() != NO_HANDLE;
  const madeBirth = birth.done() != NO_HANDLE;
  const madeRefused = Trigger.on(Event.OrderRefused).enqueue(onNotCarriedOut).done() != NO_HANDLE;
  if (!madeCommanded || !madeBirth || !madeRefused) log("ai-takeover: a trigger could not be made");
}

/** A unit took an order. */
function onCommanded(r: EventRecord): void {
  const seat = seatOfOwner(r.subject);
  if (seat < 0) return;
  if (r.value == <i64>Command.AttackMove && attackFrame[seat] != 0) {
    mark(MARK_ATTACK, "DEMO-OK attack");
  } else if (r.value == <i64>Command.DeployMcv) {
    mark(NOTE_DEPLOY_HEARD, "ai-takeover: the MCV took the deploy order");
  }
}

/** An order of a claimed house was not carried out. */
function onNotCarriedOut(r: EventRecord): void {
  const seat = seatOf(r.house);
  if (seat < 0) return;
  mark(MARK_REJECTED, "DEMO-OK order-rejected");
  if (r.value == <i64>Command.PlaceReadyBuilding && r.value2 == REASON_CANNOT_PLACE) {
    placeFails[seat] += 1;
    placingType[seat] = 0;
  }
  if (detailLines < MAX_DETAIL_LINES) {
    detailLines++;
    log("ai-takeover: frame " + r.frame.toString() + " order " + r.value.toString() + " not carried out: " +
      reasonName(r.value2));
  }
}

/** A unit left a factory. */
function onBirth(r: EventRecord): void {
  const unit = r.subject;
  const seat = seatOfOwner(unit);
  if (seat < 0 || training[seat] == 0) return;
  const side = seatSideOf[seat];
  const kind = getOr(unit, Prop.Type, 0);
  if (kind == ty(side, SLOT_INFANTRY) || kind == ty(side, SLOT_VEHICLE)) mark(MARK_TRAIN, "DEMO-OK train");
}

/** `RefuseReason` names (appendix 8.3, enum 26). */
function reasonName(reason: i64): string {
  switch (<i32>reason) {
    case 1: return "gone";
    case 2: return "ignores orders";
    case 3: return "illegal target";
    case 4: return "prerequisite";
    case 5: return "cannot afford";
    case 6: return "queue full";
    case 7: return "cannot place";
    case 8: return "not ready";
    case 9: return "vetoed";
    case 10: return "rate limited";
    default: return "other";
  }
}

// ------------------------------------------------------------- The driver

function start(): void {
  resolveTypes();
  makeTriggers();
  claimSeats();
}

function tick(): void {
  const now = <i64>frame();
  const count = seatCount();
  if (count == 0) {
    // The houses may not hold their MCVs yet; look again for a while.
    if (now <= 300) claimSeats();
    else mark(NOTE_NO_SEAT, "ai-takeover: no computer house to play");
    return;
  }
  const human: House = Find.houses().where(Prop.IsHuman, Op.Eq, 1).first();
  if (human != NO_HANDLE && getOr(human, Prop.IsDefeated, 0) != 0) {
    mark(NOTE_PLAYER_DEFEATED, "ai-takeover: the player's house is defeated");
  }
  const visit = visits;
  visits = visit + 1;
  const seat = <i32>(visit % <i64>count);
  drive(seat, now);
  if (now >= 900 && !idleChecked[seat]) checkNativeIdle(seat);
  if (now % 600 == 0) status(seat, now);
}

/** One visit of a claimed house. */
function drive(seat: i32, now: i64): void {
  const house = seatHouseOf[seat];
  if (getOr(house, Prop.IsDefeated, 1) != 0) return;
  const side = seatSideOf[seat];
  const yard = owned(house, ty(side, SLOT_YARD));
  if (yard == NO_HANDLE) {
    deploy(seat, house, side, now);
    return;
  }
  if (deployFrame[seat] != 0) mark(MARK_DEPLOY, "DEMO-OK deploy");
  const yardCell = get(yard, Prop.Cell);
  if (lastStatus() != Status.Ok) return;
  // An order it cannot carry out yet, on purpose: a tank before any war
  // factory. The execution step answers `OrderRefused` (Prerequisite).
  if (!provoked[seat] && !standing(house, ty(side, SLOT_BUILD + STEP_WAR_FACTORY))) {
    const tank = ty(side, SLOT_VEHICLE);
    if (tank != NO_HANDLE && House.order(house, Order.queueProduction(tank)) == Status.Ok) provoked[seat] = true;
  }
  build(seat, house, side, yardCell);
  train(seat, house, side);
  attack(seat, house, side, now);
}

/** `v` clamped to a cell coordinate (Rust's `u16` saturating arithmetic). */
function clampCoord(v: i32): i32 {
  return v < 0 ? 0 : v > 65535 ? 65535 : v;
}

/** Deploys the house's MCV, moving it a little when deploying keeps failing where it stands. */
function deploy(seat: i32, house: House, side: i32, now: i64): void {
  const mcv = owned(house, ty(side, SLOT_MCV));
  if (mcv == NO_HANDLE) return;
  const last = deployFrame[seat];
  if (last != 0 && now - last < 60) return;
  const tries = deployTries[seat];
  deployTries[seat] = tries + 1;
  ONE_UNIT[0] = mcv;
  if (tries > 0 && tries % 3 == 0) {
    // Three tries where it stands: try a few cells away.
    const at = get(mcv, Prop.Cell);
    if (lastStatus() == Status.Ok) {
      const step = <i32>(3 * (tries / 3));
      const to = (tries / 3) % 2 == 1
        ? Cell.at(clampCoord(Cell.x(at) + step), clampCoord(Cell.y(at) + step))
        : Cell.at(clampCoord(Cell.x(at) - step), clampCoord(Cell.y(at) - step));
      House.order(house, Order.moveTo(ONE_UNIT, to));
      deployFrame[seat] = max(now, 1);
      return;
    }
  }
  if (House.order(house, Order.deployMcv(ONE_UNIT)) == Status.Ok) deployFrame[seat] = max(now, 1);
}

/** The building factory: place what is ready, else start the next step of the build order the house does not own yet. */
function build(seat: i32, house: House, side: i32, yardCell: Cell): void {
  const ready = getOr(house, Prop.Factory_Building_Ready, 0);
  const queued = getOr(house, Prop.Factory_Building_Queued, 0);
  if (ready != 0) {
    mark(MARK_BUILD, "DEMO-OK build");
    const product: TypeRef = getOr(house, Prop.Factory_Building_Type, 0);
    if (product != NO_HANDLE) place(seat, house, product, yardCell);
    return;
  }
  const placing: TypeRef = placingType[seat];
  if (placing != NO_HANDLE) {
    const at: Cell = placingCell[seat];
    const placed = Find.objects().ownedBy(house).ofType(placing).near(at, 6 * 256).first();
    if (placed != NO_HANDLE) mark(MARK_PLACE, "DEMO-OK place");
    placingType[seat] = 0;
  }
  if (queued != 0) return;
  for (let step = 0; step < BUILD_STEPS; step++) {
    const building = ty(side, SLOT_BUILD + step);
    if (building == NO_HANDLE || standing(house, building)) continue;
    House.order(house, Order.queueProduction(building));
    return;
  }
  mark(MARK_BASE, "DEMO-OK base");
}

/** Places the finished `building` at a `Placeable` cell near the yard: about sqrt(`target`) cells out, further after each failed placement. */
function place(seat: i32, house: House, building: TypeRef, yardCell: Cell): void {
  const fails = min(placeFails[seat], 8);
  const target = 20 + 12 * fails;
  const find = Find.cells()
    .near(yardCell, 12 * 256)
    .ofType(building)
    .seenBy(house)
    .where(Prop.Placeable, Op.Eq, 1);
  let after: i64 = 0;
  let best: Cell = NO_HANDLE;
  let bestScore: i64 = i64.MAX_VALUE;
  for (let n = 0; n < 8; n++) {
    const page = find.page(CELLS, after);
    if (lastStatus() != Status.Ok) break;
    const count = page.count;
    const next = page.next;
    for (let i = 0; i < count; i++) {
      const cell: Cell = CELLS[i];
      const dx = <i64>Cell.x(cell) - <i64>Cell.x(yardCell);
      const dy = <i64>Cell.y(cell) - <i64>Cell.y(yardCell);
      const score = abs(dx * dx + dy * dy - target);
      if (score < bestScore) {
        best = cell;
        bestScore = score;
      }
    }
    if (next == 0) break;
    after = next;
  }
  if (best == NO_HANDLE) {
    mark(NOTE_NO_CELL, "ai-takeover: no cell to place a building yet");
    return;
  }
  if (House.order(house, Order.placeReadyBuilding(building, best)) == Status.Ok) {
    placingType[seat] = building;
    placingCell[seat] = best;
  }
}

/** Infantry from the barracks, tanks from the war factory; a ready unit is let out when it is still waiting on the next visit. */
function train(seat: i32, house: House, side: i32): void {
  for (let line = 0; line < 2; line++) {
    const infantry = line == 0;
    const step = infantry ? STEP_BARRACKS : STEP_WAR_FACTORY;
    const slot = infantry ? SLOT_INFANTRY : SLOT_VEHICLE;
    const kind = infantry ? KIND_INFANTRY : KIND_VEHICLE;
    const queuedProp = infantry ? Prop.Factory_Infantry_Queued : Prop.Factory_Vehicle_Queued;
    const readyProp = infantry ? Prop.Factory_Infantry_Ready : Prop.Factory_Vehicle_Ready;
    const depth: i64 = infantry ? 2 : 1;
    if (!standing(house, ty(side, SLOT_BUILD + step))) continue;
    const ready = getOr(house, readyProp, 0) != 0;
    const wasReady = infantry ? infantryReady[seat] : vehicleReady[seat];
    let nowReady = ready;
    if (ready && wasReady) {
      House.order(house, Order.takeProduct(kind));
      nowReady = false;
    }
    if (infantry) infantryReady[seat] = nowReady;
    else vehicleReady[seat] = nowReady;
    const queued = getOr(house, queuedProp, 0);
    const trained = training[seat];
    const unit = ty(side, slot);
    if (queued < depth && trained < MAX_TRAINED && unit != NO_HANDLE &&
      House.order(house, Order.queueProduction(unit)) == Status.Ok) {
      training[seat] = trained + 1;
    }
  }
}

/** Sends the army at the human player's base once the war factory stands and the army is large enough, and again every `ATTACK_EVERY` frames. */
function attack(seat: i32, house: House, side: i32, now: i64): void {
  const last = attackFrame[seat];
  if (last != 0 && now - last < ATTACK_EVERY) return;
  if (!standing(house, ty(side, SLOT_BUILD + STEP_WAR_FACTORY))) return;
  let len = collect(house, ty(side, SLOT_INFANTRY), 0);
  len = collect(house, ty(side, SLOT_VEHICLE), len);
  if (len < ATTACK_SIZE) return;
  const target = enemyBase();
  if (target == NO_HANDLE) return;
  if (House.order(house, Order.attackMove(armyOf(len), target)) == Status.Ok) {
    if (last == 0) {
      log("ai-takeover: attack-move with " + len.toString() + " units to " + Cell.x(target).toString() + "," +
        Cell.y(target).toString());
    }
    attackFrame[seat] = max(now, 1);
  }
}

/** Evidence that the native AI stood down: every building the house owns is its yard or a step of the module's build order. */
function checkNativeIdle(seat: i32): void {
  const house = seatHouseOf[seat];
  const side = seatSideOf[seat];
  if (owned(house, ty(side, SLOT_YARD)) == NO_HANDLE) return;
  idleChecked[seat] = true;
  const find = Find.objects().ownedBy(house).where(Prop.Category, Op.Eq, <i64>Category.Building);
  let after: i64 = 0;
  let buildings = 0;
  let foreign = 0;
  while (true) {
    const page = find.page(BUILDINGS, after);
    if (lastStatus() != Status.Ok) return;
    const count = page.count;
    const next = page.next;
    for (let i = 0; i < count; i++) {
      buildings++;
      const kind = getOr(BUILDINGS[i], Prop.Type, 0);
      let ordered = kind == ty(side, SLOT_YARD);
      for (let step = 0; step < BUILD_STEPS; step++) if (kind == ty(side, SLOT_BUILD + step)) ordered = true;
      if (!ordered) foreign++;
    }
    if (next == 0) break;
    after = next;
  }
  if (foreign == 0 && buildings > 0) mark(MARK_IDLE, "DEMO-OK native-idle");
  else if (foreign > 0) mark(NOTE_NATIVE_BUILT, "ai-takeover: the house owns a building the module did not order");
}

function status(seat: i32, now: i64): void {
  const house = seatHouseOf[seat];
  const side = seatSideOf[seat];
  const page = Find.objects().ownedBy(house).where(Prop.Category, Op.Eq, <i64>Category.Building).page(BUILDINGS, 0);
  const built: i64 = lastStatus() == Status.Ok ? <i64>page.count : -1;
  let len = collect(house, ty(side, SLOT_INFANTRY), 0);
  len = collect(house, ty(side, SLOT_VEHICLE), len);
  log("ai-takeover: frame " + now.toString() + " seat " + seat.toString() + " buildings " + built.toString() +
    " army " + len.toString() + " credits " + getOr(house, Prop.Money, -1).toString());
}

// ------------------------------------------------------------ Registration

onScenarioStart(start);
onFrame(new Every(15), tick);
