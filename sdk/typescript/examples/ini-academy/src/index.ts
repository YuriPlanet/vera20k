// "INI Academy", a demo `rules.wasm` for INI names a module registers
// (RFC 0001 section 6h), the same as the Rust SDK's `sdk/examples/ini-academy`.
// The modder configures everything in the mod's `rulesmd.ini`; the module
// only implements it:
//
// - `[Demo.AcademyTypes]`, a registry: each entry is a section with
//   `Rank=` (`Rookie`, `Veteran` or `Elite`; default `Elite`), `Allowed=`
//   (the TechnoTypes that may be promoted; empty for any) and `Eject=`
//   (yes: the building throws its occupants back out after a promotion).
// - `Demo.Academy=<academy type>` on a garrisonable building's TechnoType
//   makes it an academy. The module binds a `PassengerAdded` trigger to
//   the type; when an occupier garrisons it (`garrison` 1), the occupant
//   gets the academy's rank.
// - `Event=Wasm:Demo.Promoted`, an object event raised on the promoted
//   unit (`Other` is the building, `value` the new rank: 0 Rookie,
//   1 Veteran, 2 Elite).
// - `Wasm:academyHello(target,text)`, a module action that greets its
//   target with the parameter text.
//
// Eject orders the building's `Unload` mission, as a player's deploy order
// on a garrisoned building does: it lets every occupant out.
//
// To exercise itself without a player, it places a Battle Bunker and a few
// infantrymen of the human player near their base and orders them in (the
// garrison click's Capture mission, aimed at the bunker). It logs
// `DEMO-OK <feature>` the first time each feature works (the engine test
// `sim::ext_modules::tests::ini_demo_mods_reach_their_marks` checks them).
import {
  Action, Cell, Event, EventRecord, EventScope, Find, House, Ini, IniEntryRecord, IniKeyRecord,
  IniSection, Mission, ModuleEvent, NO_HANDLE, NO_ID, Op, PassengerAddedEvent, Prop, Rules,
  SpeedType, Status, Target, Techno, Trigger, TriggerRef, TypeRef,
  Every, fail, frame, get, lastStatus, log, namedAction, onFrame, onScenarioStart, set, text,
} from "@vera20k/sdk";

/** `Rank` values (appendix 7.1). */
const ROOKIE: i64 = 0;
const VETERAN: i64 = 1;
const ELITE: i64 = 2;

// ------------------------------------------------------------------ Memory
// Module memory is saved and hashed with the match.

/** Academy types from `[Demo.AcademyTypes]`, in list order. */
const MAX_KINDS: i32 = 16;
let kindCount: i32 = 0;
/** The entry section's name as a string ID. */
const kindName = new StaticArray<i32>(MAX_KINDS);
const kindRank = new StaticArray<i64>(MAX_KINDS);
const kindEject = new StaticArray<bool>(MAX_KINDS);
/**
 * `Allowed=`: up to `MAX_ALLOWED` TechnoType IDs per academy type, in
 * `kindAllowed[kind * MAX_ALLOWED..]`; a count of 0 allows any unit.
 */
const MAX_ALLOWED: i32 = 16;
const kindAllowedCount = new StaticArray<i32>(MAX_KINDS);
const kindAllowed = new StaticArray<i32>(MAX_KINDS * MAX_ALLOWED);

/** Building types with `Demo.Academy=`: type ID and academy type index. */
const MAX_TYPES: i32 = 64;
let typeCount: i32 = 0;
const typeId = new StaticArray<i32>(MAX_TYPES);
const typeKind = new StaticArray<i32>(MAX_TYPES);

/** The one `PassengerAdded` trigger, bound to every academy building type. */
let enterTrigger: TriggerRef = NO_HANDLE;
/** `Demo.Promoted`, set in initialization (`NO_ID` when refused). */
let promoted: ModuleEvent = NO_ID;

/** Promotions and greetings so far; only the first few are logged. */
let promotions: i32 = 0;
let greetings: i32 = 0;
const LOG_FIRST: i32 = 4;

/** The staged scene: done, attempts so far, the recruits and when they are next ordered in. */
let staged: bool = false;
let stageTries: i32 = 0;
const MAX_STAGE_TRIES: i32 = 20;
let academy: Techno = NO_HANDLE;
const RECRUITS: StaticArray<string> = ["E1", "E2", "E1", "E2"];
const recruit = new StaticArray<Techno>(4);
let orderAt: i32 = 0;
/** Recruits not yet in are ordered in again every so often until then. */
const REORDER_EVERY: i32 = 150;
const REORDER_UNTIL: i32 = 2400;

/** Construction yards and MCVs of the three sides: where a house's base is. */
const BASES: StaticArray<string> = ["GACNST", "NACNST", "YACNST", "AMCV", "SMCV", "PCV"];
/** The building the scene uses: the Soviet Battle Bunker (a 2x2 garrison). */
const BUNKER = "NABNKR";
const HOUSES = new StaticArray<i64>(16);

/** `DEMO-OK` lines already logged, one bit each, and other one-time lines. */
let marks: i32 = 0;
const MARK_ENTRY = 1;
const MARK_KEY = 2;
const MARK_ENTER = 4;
const MARK_PROMOTE = 8;
const MARK_EVENT = 16;
const MARK_ACTION = 32;
const NOTE_EJECT = 64;
const NOTE_EJECT_FAILED = 128;
const NOTE_RAISE_FAILED = 256;
const NOTE_RANK_FAILED = 512;

/** Logs `line` the first time `bit` is marked. */
function mark(bit: i32, line: string): void {
  if ((marks & bit) == 0) {
    marks |= bit;
    log(line);
  }
}

/** Whether `bit` is still unmarked; marks it. */
function first(bit: i32): bool {
  const was = marks;
  marks |= bit;
  return (was & bit) == 0;
}

// --------------------------------------------------- INI records at start

/** `IniEntry`: one academy type of `[Demo.AcademyTypes]`. */
function loadAcademyType(r: IniEntryRecord): void {
  const section = r.entryName;
  const count = kindCount;
  if (count == MAX_KINDS) bad(section, "", "is past the 16 academy types this module holds");
  const rank = readRank(section);
  const eject = readEject(section);
  const allowed = readAllowed(section, count);
  kindName[count] = r.record.args;
  kindRank[count] = rank;
  kindEject[count] = eject;
  kindAllowedCount[count] = allowed;
  kindCount = count + 1;
  log("ini-academy: academy type " + section + " rank " + rankName(rank) + ", " +
    (allowed == 0 ? "any unit" : allowed.toString() + " allowed types") +
    (eject ? ", eject" : ""));
  mark(MARK_ENTRY, "DEMO-OK entry");
}

/** `[section] Rank=`: `Rookie`, `Veteran` or `Elite` (or 0, 1, 2), ignoring case; `Elite` when missing or empty. */
function readRank(section: string): i64 {
  let value = Rules.get(section, "Rank");
  const status = lastStatus();
  if (status == Status.NotFound) value = "";
  else if (status != Status.Ok) bad(section, "Rank", "could not be read");
  value = trim(value);
  if (!value.length) return ELITE;
  if (sameAscii(value, "Rookie")) return ROOKIE;
  if (sameAscii(value, "Veteran")) return VETERAN;
  if (sameAscii(value, "Elite")) return ELITE;
  const n = Ini.int(value, -1);
  if (lastStatus() == Status.Ok && n >= ROOKIE && n <= ELITE) return n;
  bad(section, "Rank", "must be Rookie, Veteran or Elite");
  return ELITE;
}

/** `[section] Eject=`: yes or no; no when missing or empty. */
function readEject(section: string): bool {
  const value = Rules.get(section, "Eject");
  const status = lastStatus();
  if (status == Status.NotFound) return false;
  if (status != Status.Ok) bad(section, "Eject", "could not be read");
  if (!trim(value).length) return false;
  const yes = Ini.yesNo(value);
  if (lastStatus() != Status.Ok) bad(section, "Eject", "must be yes or no");
  return yes;
}

/** `[section] Allowed=` into academy type `kind`'s list; how many types it names (0: any unit). */
function readAllowed(section: string, kind: i32): i32 {
  let value = Rules.get(section, "Allowed");
  const status = lastStatus();
  if (status == Status.NotFound) value = "";
  else if (status != Status.Ok) bad(section, "Allowed", "could not be read");
  const names = Ini.split(value);
  let count = 0;
  for (let i = 0; i < names.length; i++) {
    const name = names[i];
    if (!name.length) continue;
    if (count == MAX_ALLOWED) bad(section, "Allowed", "names more than 16 types");
    const t = Rules.technoType(name);
    if (t == NO_HANDLE) bad(section, "Allowed", "names " + name + ", which is no TechnoType");
    kindAllowed[kind * MAX_ALLOWED + count] = TypeRef.id(t);
    count++;
  }
  return count;
}

/** `IniKey`: a building type with `Demo.Academy=`. */
function attachAcademy(r: IniKeyRecord): void {
  let section = r.sectionName;
  if (!section.length) section = "?";
  const kind = kindNamed(r.record.args);
  if (kind < 0) bad(section, "Demo.Academy", "names no section listed in [Demo.AcademyTypes]");
  const count = typeCount;
  if (count == MAX_TYPES) bad(section, "Demo.Academy", "is past the 64 academy buildings this module holds");
  const t = r.typeRef;
  if (enterTrigger == NO_HANDLE) {
    enterTrigger = Trigger.on(Event.PassengerAdded).enqueue(onEnter).done();
    if (enterTrigger == NO_HANDLE) fail("ini-academy: the PassengerAdded trigger could not be made");
  }
  if (TriggerRef.bind(enterTrigger, t) != Status.Ok) {
    bad(section, "Demo.Academy", "could not bind the academy to this type");
  }
  typeId[count] = TypeRef.id(t);
  typeKind[count] = kind;
  typeCount = count + 1;
  mark(MARK_KEY, "DEMO-OK key");
}

/** The academy type whose entry name is string `id`, ignoring case; −1 for none. */
function kindNamed(id: i32): i32 {
  for (let i = 0; i < kindCount; i++) if (kindName[i] == id) return i;
  const want = trim(text(id));
  if (lastStatus() != Status.Ok) return -1;
  for (let i = 0; i < kindCount; i++) {
    const have = text(kindName[i]);
    if (lastStatus() == Status.Ok && sameAscii(have, want)) return i;
  }
  return -1;
}

/** Logs what is wrong with `[section] key=` and stops the module (a trap stops only this module, RFC section 3). */
function bad(section: string, key: string, what: string): void {
  fail("ini-academy: [" + section + "] " + (key.length ? key + "= " : "") + what);
}

function rankName(rank: i64): string {
  if (rank == ROOKIE) return "Rookie";
  if (rank == VETERAN) return "Veteran";
  return "Elite";
}

// ------------------------------------------------------------------ Start

function start(): void {
  log("ini-academy: " + kindCount.toString() + " academy types on " + typeCount.toString() + " building types");
}

// --------------------------------------------------------------- Academy

/** The academy type of building type `id`, or −1 when it is no academy. */
function kindOfType(id: i32): i32 {
  for (let i = 0; i < typeCount; i++) if (typeId[i] == id) return typeKind[i];
  return -1;
}

/** Whether academy type `kind` may promote a unit of type `id`. */
function allowed(kind: i32, id: i32): bool {
  const count = kindAllowedCount[kind];
  if (count == 0) return true;
  for (let i = 0; i < count; i++) if (kindAllowed[kind * MAX_ALLOWED + i] == id) return true;
  return false;
}

/** The `PassengerAdded` trigger's module action: an academy building took an occupant. */
function onEnter(e: PassengerAddedEvent): void {
  // Transports load passengers too; only a garrison counts.
  if (e.garrison != 1) return;
  mark(MARK_ENTER, "DEMO-OK enter");
  const building = e.transport;
  const unit = e.passenger;
  const buildingType = Techno.typeRef(building);
  if (lastStatus() != Status.Ok) return;
  const kind = kindOfType(TypeRef.id(buildingType));
  if (kind < 0) return;
  const unitType = Techno.typeRef(unit);
  if (lastStatus() != Status.Ok) return; // Gone again before the module phase.
  if (!allowed(kind, TypeRef.id(unitType))) return;
  const rank = kindRank[kind];
  // Never a demotion: a unit at the academy's rank or above keeps its own.
  const current = get(unit, Prop.Rank);
  if (lastStatus() != Status.Ok || current >= rank) return;
  if (set(unit, Prop.Rank, rank) != Status.Ok) {
    if (first(NOTE_RANK_FAILED)) log("ini-academy: a rank could not be written");
    return;
  }
  let now = get(unit, Prop.Rank);
  const readBack = lastStatus() == Status.Ok;
  if (rank == ELITE && readBack && now == ELITE) mark(MARK_PROMOTE, "DEMO-OK promote");
  if (!readBack) now = rank;
  promotions++;
  if (promotions <= LOG_FIRST) log("ini-academy: a unit entered an academy and is now " + rankName(now));
  const raised = Action.raiseEvent(promoted, unit, building, rank, 0);
  if (raised == Status.Ok) mark(MARK_EVENT, "DEMO-OK event");
  else if (first(NOTE_RAISE_FAILED)) log("ini-academy: Demo.Promoted could not be raised: " + statusName(raised));
  if (kindEject[kind]) {
    // The building's owner orders it, as a player's deploy order does.
    const status = Techno.setMission(building, Mission.Unload);
    if (status == Status.Ok) {
      if (first(NOTE_EJECT)) log("ini-academy: the academy lets its occupants out");
    } else if (first(NOTE_EJECT_FAILED)) {
      log("ini-academy: the academy could not let its occupants out: " + statusName(status));
    }
  }
}

/** Module action `academyHello(target,text)`: greets its target. */
function academyHello(r: EventRecord): void {
  const words = r.argsText;
  const target = r.target;
  let unit: Techno = NO_HANDLE;
  if (target == Target.Subject) unit = r.subject;
  else if (target == Target.Other) unit = r.other;
  else if (target == Target.TagHolder) unit = r.holder;
  if (words.length) mark(MARK_ACTION, "DEMO-OK action");
  greetings++;
  if (greetings <= LOG_FIRST) {
    let rank = get(unit, Prop.Rank);
    if (lastStatus() != Status.Ok) rank = ROOKIE;
    log("ini-academy: academyHello says \"" + words + "\" to a new " + rankName(rank));
  }
}

// ------------------------------------------------------- The staged scene

/** The cell of `house`'s construction yard or MCV, or `NO_HANDLE`. */
function baseOf(house: House): Cell {
  for (let i = 0; i < BASES.length; i++) {
    const t = Rules.technoType(BASES[i]);
    if (t == NO_HANDLE) continue;
    const object = Find.objects().ownedBy(house).ofType(t).first();
    if (object != NO_HANDLE) {
      const cell = get(object, Prop.Cell);
      if (lastStatus() == Status.Ok) return cell;
    }
  }
  return NO_HANDLE;
}

/** The first computer house with a base, where the scene leans to; `NO_HANDLE` for none. */
function enemyBase(): Cell {
  const page = Find.houses().where(Prop.IsHuman, Op.Eq, 0).page(HOUSES, 0);
  if (lastStatus() != Status.Ok) return NO_HANDLE;
  const count = page.count;
  for (let i = 0; i < count; i++) {
    const cell = baseOf(HOUSES[i]);
    if (cell != NO_HANDLE) return cell;
  }
  return NO_HANDLE;
}

/** Whether `(x, y)` is on the map's range. */
function onRange(x: i32, y: i32): bool {
  return x >= 1 && x <= 500 && y >= 1 && y <= 500;
}

/** Whether the 2x2 block from `(x, y)` is open ground. */
function openBlock(x: i32, y: i32): bool {
  for (let dy = 0; dy < 2; dy++) {
    for (let dx = 0; dx < 2; dx++) {
      if (!onRange(x + dx, y + dy)) return false;
      const cell = Cell.at(x + dx, y + dy);
      if (!Cell.passable(cell, SpeedType.Track) || lastStatus() != Status.Ok) return false;
    }
  }
  return true;
}

function stage(): void {
  const tries = stageTries + 1;
  if (tries > MAX_STAGE_TRIES) return;
  stageTries = tries;
  if (!placeScene() && tries == MAX_STAGE_TRIES) log("ini-academy: found no place to stage the scene");
}

/** Places a bunker of the human player's a quarter of the way from their base towards the computer's, and the recruits around it. */
function placeScene(): bool {
  const human: House = Find.houses().where(Prop.IsHuman, Op.Eq, 1).first();
  if (human == NO_HANDLE) return false;
  const home = baseOf(human);
  if (home == NO_HANDLE) return false;
  const bunker = Rules.technoType(BUNKER);
  const e1 = Rules.technoType("E1");
  if (bunker == NO_HANDLE || e1 == NO_HANDLE) {
    log("ini-academy: the rules have no NABNKR or E1");
    stageTries = MAX_STAGE_TRIES;
    return false;
  }
  let away = enemyBase();
  if (away == NO_HANDLE) away = home;
  const hx = Cell.x(home);
  const hy = Cell.y(home);
  const ax = hx + (Cell.x(away) - hx) / 4;
  const ay = hy + (Cell.y(away) - hy) / 4;
  // The bunker: the first open block in rings around that point.
  let building: Techno = NO_HANDLE;
  let bx = 0;
  let by = 0;
  for (let ring = 0; ring <= 8 && building == NO_HANDLE; ring++) {
    for (let dy = -ring; dy <= ring && building == NO_HANDLE; dy++) {
      for (let dx = -ring; dx <= ring; dx++) {
        if (max(abs(dx), abs(dy)) != ring || !openBlock(ax + dx, ay + dy)) continue;
        if (!onRange(ax + dx, ay + dy)) continue;
        const made = Action.createObject(Cell.at(ax + dx, ay + dy), bunker, human);
        if (made != NO_HANDLE) {
          building = made;
          bx = ax + dx;
          by = ay + dy;
          break;
        }
      }
    }
  }
  if (building == NO_HANDLE) return false;
  academy = building;
  // The recruits: open cells two to five cells from the bunker.
  let made = 0;
  for (let ring = 2; ring <= 5 && made < RECRUITS.length; ring++) {
    for (let dy = -ring; dy <= ring + 1 && made < RECRUITS.length; dy++) {
      for (let dx = -ring; dx <= ring + 1 && made < RECRUITS.length; dx++) {
        const edge = dx == -ring || dy == -ring || dx == ring + 1 || dy == ring + 1;
        if (!edge || !onRange(bx + dx, by + dy)) continue;
        const cell = Cell.at(bx + dx, by + dy);
        if (!Cell.passable(cell, SpeedType.Foot) || lastStatus() != Status.Ok) continue;
        const kind = Rules.technoType(RECRUITS[made]);
        if (kind == NO_HANDLE) {
          made++;
          continue;
        }
        // An occupied cell takes no unit; the next one is tried.
        const unit = Action.createObject(cell, kind, human);
        if (unit != NO_HANDLE) {
          recruit[made] = unit;
          made++;
        }
      }
    }
  }
  staged = true;
  // Ordered in from the next pass, once the new objects have settled.
  orderAt = frame() + 15;
  log("ini-academy: a Battle Bunker academy and " + made.toString() + " recruits near (" +
    bx.toString() + "," + by.toString() + ")");
  return true;
}

/** Orders the recruits not yet inside, and not yet Elite, into the bunker. */
function orderRecruits(now: i32): void {
  Techno.health(academy);
  if (lastStatus() != Status.Ok) {
    orderAt = REORDER_UNTIL + 1;
    return;
  }
  for (let i = 0; i < recruit.length; i++) {
    const unit = recruit[i];
    if (unit == NO_HANDLE) continue;
    Techno.health(unit);
    if (lastStatus() != Status.Ok) continue;
    const transport = Techno.transport(unit);
    const inside = lastStatus() == Status.Ok && transport != NO_HANDLE;
    const rank = get(unit, Prop.Rank);
    const elite = lastStatus() == Status.Ok && rank == ELITE;
    if (!inside && !elite) {
      // A player's garrison click is the Capture order with the building
      // as its destination (`Command.CaptureBuilding`).
      Action.setMissionOn(unit, Mission.Capture, academy);
    }
  }
  orderAt = now + REORDER_EVERY;
}

// ------------------------------------------------------------------ Frame

function tick(): void {
  const now = frame();
  if (!staged) {
    if (now >= 30 && now % 15 == 0) stage();
    return;
  }
  if (now <= REORDER_UNTIL && now >= orderAt) orderRecruits(now);
}

// ------------------------------------------------------------------- Text

function isSpace(c: i32): bool {
  return c == 32 || c == 9 || c == 10 || c == 12 || c == 13;
}

/** `s` without surrounding ASCII spaces (Rust's `trim_ascii`). */
function trim(s: string): string {
  let start = 0;
  let end = s.length;
  while (start < end && isSpace(s.charCodeAt(start))) start++;
  while (end > start && isSpace(s.charCodeAt(end - 1))) end--;
  return s.substring(start, end);
}

/** Whether `a` and `b` are equal ignoring ASCII case (Rust's `eq_ignore_ascii_case`). */
function sameAscii(a: string, b: string): bool {
  if (a.length != b.length) return false;
  for (let i = 0; i < a.length; i++) {
    let x = a.charCodeAt(i);
    let y = b.charCodeAt(i);
    if (x >= 65 && x <= 90) x += 32;
    if (y >= 65 && y <= 90) y += 32;
    if (x != y) return false;
  }
  return true;
}

/** A status's name. The log never says "refused" (the demo test looks for it). */
function statusName(status: Status): string {
  if (status == Status.Gone) return "gone";
  if (status == Status.NotFound) return "not found";
  if (status == Status.Refused) return "declined";
  if (status == Status.Unsupported) return "unsupported";
  return "ok";
}

// ------------------------------------------------------------ Registration
// Top-level code is the module's initialization, where INI names register.

if (Ini.registry("Demo.AcademyTypes", loadAcademyType) != Status.Ok) {
  log("ini-academy: the name Demo.AcademyTypes is taken");
}
if (Ini.key("Demo.Academy", IniSection.TechnoType, attachAcademy) != Status.Ok) {
  log("ini-academy: the name Demo.Academy is taken");
}
promoted = Ini.event("Demo.Promoted", EventScope.Object);
if (promoted == NO_ID) log("ini-academy: the name Demo.Promoted is taken");

// The INI trigger `[AcademyGraduation]` calls it (`Action1=Wasm:academyHello(Subject,welcome)`).
namedAction("academyHello", academyHello);
onScenarioStart(start);
onFrame(new Every(1), tick);
